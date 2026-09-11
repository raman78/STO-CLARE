//! The one shortcut taken from the whole desktop.
//!
//! The overlay's key has to work while the *game* has the screen, which no
//! ordinary key press does: a window only hears the keyboard while it is in
//! front. So the key is taken from the desktop itself, on a thread of its own,
//! and arrives here as a press on a channel.
//!
//! ## Why an X11 grab and not the portal
//!
//! On Linux there are two ways to ask for a key, and the choice was measured on
//! KWin/Wayland rather than argued:
//!
//! - **A passive X11 grab** (`XGrabKey`), which is what this module does. It
//!   needs an X server — under Wayland that is XWayland, which every desktop
//!   that runs a Proton game already has, the game being an X client itself.
//!   Measured with Star Trek Online in front: the key reached the grab. It also
//!   reached it with a *native Wayland* window in front, so the grab is global
//!   in practice and not merely global among X windows.
//! - **`org.freedesktop.portal.GlobalShortcuts`**, the protocol answer. It
//!   works — session, bind, and the press arrives — but it costs a D-Bus client
//!   in the dependency tree, it refuses a program that has not claimed an app
//!   id through `org.freedesktop.host.portal.Registry`, and once bound **the
//!   program cannot change its own key**: re-binding with a different
//!   combination is accepted and ignored, and the reader has to go to the
//!   desktop's own settings. That last one is what settles it, because the
//!   shortcut is meant to be editable in our own Settings window.
//!
//! The portal is where to go if a session ever turns up with no XWayland in it.
//! Until then the grab is fewer moving parts and the key stays ours to rebind.
//!
//! ## What a grab takes
//!
//! A grab takes the key **away from whoever has focus** — including the game,
//! and including our own window. That is the point (the game must not also act
//! on it), and it is why the in-window half of this shortcut is skipped while
//! the grab holds the key (see [`super::Shortcuts::triggered`]). It is also why
//! only one action can be taken this way: every key taken here is a key no
//! other program can use.

use std::{
    sync::{Arc, atomic::Ordering},
    thread,
    time::Duration,
};

use crossbeam_channel::Receiver;
use eframe::egui::Context;

use super::Combination;

/// What became of the desktop-wide key. The settings tab states it, because
/// every case but the first is a shortcut that will not answer, and a key that
/// does nothing with nothing said is indistinguishable from a broken program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobalState {
    /// Not asked for.
    Off,
    /// Held: the desktop sends this key here wherever it is pressed.
    Held(String),
    /// Asked for, and refused — another program holds that key, or the X server
    /// has no such key on the current layout.
    Refused(String),
    /// Nothing on this session can take a key from the desktop at all.
    Unsupported(String),
}

impl GlobalState {
    /// The line the settings tab shows.
    pub fn message(&self) -> String {
        match self {
            Self::Off => "Not taken — the shortcut only works while this window is in front."
                .to_owned(),
            Self::Held(trigger) => {
                format!("{trigger} is taken from the whole desktop; it works while the game is in front.")
            }
            Self::Refused(reason) => format!("Could not be taken: {reason}"),
            Self::Unsupported(reason) => format!("Not available here: {reason}"),
        }
    }

    /// Whether this is worth a warning colour: the reader asked for a key and
    /// has not got it.
    pub fn is_problem(&self) -> bool {
        matches!(self, Self::Refused(_) | Self::Unsupported(_))
    }
}

/// A key held against the desktop: the presses it sends, and how to give it
/// back.
struct Taken {
    presses: Receiver<()>,
    /// Stops the thread and waits for it, so the key is back before the next
    /// one is asked for. A closure because how a thread is stopped differs per
    /// platform and nothing above needs to know.
    release: Box<dyn FnOnce() + Send>,
}

/// The desktop-wide shortcut, as the app holds it.
pub struct GlobalHotkey {
    /// The window is asked to repaint when a press arrives. Without it the
    /// press would sit in the channel: egui only draws when something happens
    /// to it, and a key pressed while the game is in front is precisely the
    /// case where nothing has. The analysis thread wakes the window the same
    /// way.
    ctx: Context,
    held: Option<(Combination, Taken)>,
    state: GlobalState,
    /// How many times the key has been taken again after the thread holding it
    /// ended by itself. Capped, so a grab that cannot survive is reported
    /// rather than remade at frame rate — the same shape as the overlay's
    /// `layer_restart`.
    retaken: u8,
}

/// How many times a lost grab is taken again before it is left alone and
/// reported.
const MAX_RETAKES: u8 = 3;

impl GlobalHotkey {
    /// A hotkey holding nothing yet, on the window it will wake.
    pub fn off(ctx: Context) -> Self {
        Self {
            ctx,
            held: None,
            state: GlobalState::Off,
            retaken: 0,
        }
    }

    /// Takes the wanted key, or gives back whatever is held when nothing is
    /// wanted. Asking for the key already held does nothing — this runs
    /// whenever the settings are applied, and re-taking the same key would drop
    /// it for as long as the round trip takes.
    pub fn bind(&mut self, wanted: Option<Combination>) {
        match wanted {
            None => {
                self.release();
                self.state = GlobalState::Off;
            }
            Some(combination) => {
                if self
                    .held
                    .as_ref()
                    .is_some_and(|(held, _)| *held == combination)
                {
                    return;
                }
                self.release();
                // A key the reader asked for afresh gets the full allowance
                // again, the way the overlay's button is its own clean retry.
                self.retaken = 0;
                self.take(combination);
            }
        }
    }

    /// How many presses arrived since the last frame. Counted rather than
    /// answered yes/no: two deliberate presses in one frame are two toggles,
    /// and the overlay would otherwise come back on a frame late.
    ///
    /// This is also where a grab that has **died** is noticed. The thread ends
    /// on its own if the X connection goes (the server restarting under a
    /// Wayland session takes XWayland with it), and nothing else would ever
    /// look — the key would simply stop working while the settings window went
    /// on saying it was taken.
    pub fn presses(&mut self) -> usize {
        let Some((combination, taken)) = &self.held else {
            return 0;
        };
        let combination = *combination;
        let mut presses = 0;
        loop {
            match taken.presses.try_recv() {
                Ok(()) => presses += 1,
                Err(crossbeam_channel::TryRecvError::Empty) => return presses,
                Err(crossbeam_channel::TryRecvError::Disconnected) => break,
            }
        }

        // The thread is gone. Take the key again, up to a point: one that
        // cannot be held would otherwise be asked for every frame.
        self.held = None;
        if self.retaken >= MAX_RETAKES {
            log::warn!("global shortcut: {combination} was lost {MAX_RETAKES} times; leaving it");
            self.state = GlobalState::Refused(format!(
                "{combination} kept being lost — the X server it was taken from has gone. \
                 Closing this window with Ok asks for it again"
            ));
            return presses;
        }
        self.retaken += 1;
        log::warn!(
            "global shortcut: {combination} was lost, taking it again (attempt {} of {MAX_RETAKES})",
            self.retaken
        );
        self.take(combination);
        presses
    }

    /// Whether a key is actually held right now — which is what decides
    /// [`super::Shortcuts::triggered`] skipping the in-window half.
    pub fn holds_a_key(&self) -> bool {
        self.held.is_some()
    }

    pub fn state(&self) -> &GlobalState {
        &self.state
    }

    /// Asks the desktop for a key and records what came of it.
    fn take(&mut self, combination: Combination) {
        match backend::take(combination, self.ctx.clone()) {
            Ok(taken) => {
                log::info!("global shortcut: {combination} taken from the desktop");
                self.state = GlobalState::Held(combination.to_string());
                self.held = Some((combination, taken));
            }
            Err(state) => {
                log::warn!("global shortcut: {}", state.message());
                self.state = state;
            }
        }
    }

    fn release(&mut self) {
        if let Some((combination, taken)) = self.held.take() {
            (taken.release)();
            log::info!("global shortcut: {combination} given back");
        }
    }
}

impl Drop for GlobalHotkey {
    fn drop(&mut self) {
        self.release();
    }
}

// ------------------------------------------------------------------ Linux

#[cfg(target_os = "linux")]
mod backend {
    use super::*;

    use std::sync::atomic::AtomicBool;

    use x11rb::{
        connection::Connection,
        protocol::{
            Event,
            xkb::{self, ConnectionExt as _},
            xproto::{ConnectionExt as _, GrabMode, ModMask, Window},
        },
        rust_connection::RustConnection,
    };

    /// How often the thread looks for a press. A grab delivers into the
    /// connection's queue, so this is only how long a press can sit there — a
    /// twentieth of a second, against a thread that would otherwise have to be
    /// woken from a blocking read to be stopped at all.
    const POLL: Duration = Duration::from_millis(40);

    /// How close two presses have to be to read as the keyboard repeating,
    /// used only where the X server would not tell us outright (see
    /// `detectable_repeat`). Auto-repeat runs at about thirty a second; two
    /// presses a quarter of a second apart are a person pressing twice.
    const REPEAT_GUARD: u32 = 250;

    /// Caps Lock and Num Lock are modifiers like any other as far as a grab is
    /// concerned, so a key taken without them is a key that stops working the
    /// moment either is on. Every combination of the two is taken.
    ///
    /// The two bits are the protocol's own — `LockMask` is bit 1, and Num Lock
    /// is by universal convention on `Mod2` — rather than anything the server
    /// decides, which is why they can stand as numbers here.
    const CAPS_LOCK: u16 = 1 << 1;
    const NUM_LOCK: u16 = 1 << 4;
    const LOCK_MASKS: [u16; 4] = [0, CAPS_LOCK, NUM_LOCK, CAPS_LOCK | NUM_LOCK];

    pub(super) fn take(combination: Combination, ctx: Context) -> Result<Taken, GlobalState> {
        let (ready_sender, ready) = crossbeam_channel::bounded(1);
        let (press_sender, presses) = crossbeam_channel::unbounded();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();

        let thread = thread::Builder::new()
            .name("cla-global-hotkey".to_owned())
            .spawn(move || run(combination, &ready_sender, &press_sender, &thread_stop, &ctx))
            .map_err(|error| {
                GlobalState::Unsupported(format!("the thread could not be started ({error})"))
            })?;

        match ready.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Taken {
                presses,
                release: Box::new(move || {
                    stop.store(true, Ordering::Relaxed);
                    let _ = thread.join();
                }),
            }),
            Ok(Err(state)) => {
                let _ = thread.join();
                Err(state)
            }
            Err(_) => {
                // Nothing came back. The thread is told to stop and left to it
                // rather than waited for, since whatever it is stuck in is the
                // reason there was no answer.
                stop.store(true, Ordering::Relaxed);
                Err(GlobalState::Unsupported(
                    "the X server did not answer".to_owned(),
                ))
            }
        }
    }

    /// The thread body: take the key, then report presses until told to stop.
    fn run(
        combination: Combination,
        ready: &crossbeam_channel::Sender<Result<(), GlobalState>>,
        presses: &crossbeam_channel::Sender<()>,
        stop: &AtomicBool,
        ctx: &Context,
    ) {
        let grabbed = match grab(combination) {
            Ok(grabbed) => {
                let _ = ready.send(Ok(()));
                grabbed
            }
            Err(state) => {
                let _ = ready.send(Err(state));
                return;
            }
        };

        let Grabbed {
            connection,
            root,
            keycode,
            masks,
            detectable_repeat,
        } = grabbed;

        // A held key repeats, and a toggle answering thirty times a second is a
        // flicker rather than a command. Where the server tells repeats apart
        // for us, the key being down is the whole test; where it does not, a
        // repeat arrives as a release and a press in the same instant, so the
        // time between presses is what tells them apart.
        let mut down = false;
        let mut last_press: Option<u32> = None;

        while !stop.load(Ordering::Relaxed) {
            loop {
                match connection.poll_for_event() {
                    Ok(Some(Event::KeyPress(event))) if event.detail == keycode => {
                        let repeating = down
                            || (!detectable_repeat
                                && last_press.is_some_and(|last| {
                                    event.time.wrapping_sub(last) < REPEAT_GUARD
                                }));
                        down = true;
                        last_press = Some(event.time);
                        if !repeating {
                            if presses.send(()).is_err() {
                                return; // nobody is listening any more
                            }
                            // The window is very likely behind a full-screen
                            // game and drawing nothing; without this the press
                            // waits in the channel until something else wakes
                            // it.
                            ctx.request_repaint();
                        }
                    }
                    Ok(Some(Event::KeyRelease(event))) if event.detail == keycode => down = false,
                    Ok(Some(_)) => (),
                    Ok(None) => break,
                    Err(error) => {
                        log::warn!("global shortcut: the X connection failed ({error})");
                        return;
                    }
                }
            }
            thread::sleep(POLL);
        }

        for mask in masks {
            let _ = connection.ungrab_key(keycode, root, mask);
        }
        let _ = connection.flush();
    }

    struct Grabbed {
        connection: RustConnection,
        root: Window,
        keycode: u8,
        masks: Vec<ModMask>,
        detectable_repeat: bool,
    }

    fn grab(combination: Combination) -> Result<Grabbed, GlobalState> {
        let (connection, screen) = x11rb::connect(None).map_err(|error| {
            GlobalState::Unsupported(format!(
                "there is no X server to take a key from ({error}). In a Wayland session that \
                 means XWayland, which a Proton game brings with it"
            ))
        })?;
        let root = connection.setup().roots[screen].root;

        let keysym = keysym(combination.key).ok_or_else(|| {
            GlobalState::Refused(format!("{combination} is not a key the X server can be told"))
        })?;
        let keycode = keycode(&connection, keysym).ok_or_else(|| {
            GlobalState::Refused(format!(
                "{combination} is not on the keyboard layout in use"
            ))
        })?;

        let detectable_repeat = ask_for_detectable_repeat(&connection);
        let wanted = modifier_mask(combination);

        let mut masks = Vec::with_capacity(LOCK_MASKS.len());
        for extra in LOCK_MASKS {
            let mask = ModMask::from(wanted | extra);
            let taken = connection
                .grab_key(true, root, mask, keycode, GrabMode::ASYNC, GrabMode::ASYNC)
                .map_err(|error| GlobalState::Refused(format!("{error}")))
                .and_then(|cookie| cookie.check().map_err(|error| refusal(combination, &error)));
            match taken {
                Ok(()) => masks.push(mask),
                Err(state) => {
                    // Half a grab is worse than none: the key would answer with
                    // Caps Lock on and not otherwise.
                    for mask in masks {
                        let _ = connection.ungrab_key(keycode, root, mask);
                    }
                    return Err(state);
                }
            }
        }
        let _ = connection.flush();

        Ok(Grabbed {
            connection,
            root,
            keycode,
            masks,
            detectable_repeat,
        })
    }

    /// What to say about a grab the server would not give us. `BadAccess` is
    /// the one that matters and the one with a cure, so it is named rather than
    /// left as a protocol error nobody can act on.
    fn refusal(combination: Combination, error: &x11rb::errors::ReplyError) -> GlobalState {
        let already_taken = matches!(
            error,
            x11rb::errors::ReplyError::X11Error(x11rb::x11_utils::X11Error {
                error_kind: x11rb::protocol::ErrorKind::Access,
                ..
            })
        );
        match already_taken {
            true => GlobalState::Refused(format!(
                "{combination} is already held by another program — pick a different one"
            )),
            false => GlobalState::Refused(format!("{combination} was refused ({error})")),
        }
    }

    /// Asks the server to stop faking a key release between repeats, so a held
    /// key is one press followed by more presses. Best effort: where the XKB
    /// extension is not there, the time guard above stands in.
    fn ask_for_detectable_repeat(connection: &RustConnection) -> bool {
        let ask = || -> Result<xkb::PerClientFlagsReply, Box<dyn std::error::Error>> {
            connection.xkb_use_extension(1, 0)?.reply()?;
            Ok(connection
                .xkb_per_client_flags(
                    xkb::ID::USE_CORE_KBD.into(),
                    xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT,
                    xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT,
                    0u32.into(),
                    0u32.into(),
                    0u32.into(),
                )?
                .reply()?)
        };
        match ask() {
            Ok(reply) => reply
                .supported
                .contains(xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT),
            Err(error) => {
                log::info!("global shortcut: XKB would not tell repeats apart ({error})");
                false
            }
        }
    }

    /// The X11 name of a key, for the keys [`Combination::is_supported`]
    /// allows: a letter, a digit, or a function key. Those are the three whose
    /// keysym is fixed rather than a matter of layout.
    fn keysym(key: eframe::egui::Key) -> Option<u32> {
        let name = key.name();
        let mut characters = name.chars();
        match (characters.next(), characters.next()) {
            // Latin-1: the lowercase letter is what an unshifted key carries.
            (Some(letter), None) if letter.is_ascii_uppercase() => {
                Some(u32::from(letter.to_ascii_lowercase() as u8))
            }
            (Some(digit), None) if digit.is_ascii_digit() => Some(u32::from(digit as u8)),
            // XK_F1 is 0xFFBE and the rest follow it in order.
            (Some('F'), Some(_)) => name[1..]
                .parse::<u32>()
                .ok()
                .filter(|number| (1..=24).contains(number))
                .map(|number| 0xFFBE + number - 1),
            _ => None,
        }
    }

    /// Where that key sits on the keyboard in use. The mapping is read rather
    /// than assumed, because a keysym's keycode is the layout's business.
    fn keycode(connection: &RustConnection, keysym: u32) -> Option<u8> {
        let setup = connection.setup();
        let first = setup.min_keycode;
        let count = setup.max_keycode - first + 1;
        let mapping = connection
            .get_keyboard_mapping(first, count)
            .ok()?
            .reply()
            .ok()?;
        let per_code = usize::from(mapping.keysyms_per_keycode).max(1);
        mapping
            .keysyms
            .chunks(per_code)
            .position(|symbols| symbols.contains(&keysym))
            .map(|index| first + index as u8)
    }

    /// egui's modifiers as X11 knows them. Alt is Mod1 — checked against the
    /// modifier map on the machine this was written on, where Mod1 holds
    /// `Alt_L`.
    fn modifier_mask(combination: Combination) -> u16 {
        let modifiers = combination.modifiers;
        let mut mask = 0;
        if modifiers.shift {
            mask |= ModMask::SHIFT.bits();
        }
        if modifiers.ctrl || modifiers.command {
            mask |= ModMask::CONTROL.bits();
        }
        if modifiers.alt {
            mask |= ModMask::M1.bits();
        }
        mask
    }
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
mod backend {
    use super::*;

    use std::{ptr, sync::atomic::AtomicU32};

    use windows_sys::Win32::{
        System::Threading::GetCurrentThreadId,
        UI::{
            Input::KeyboardAndMouse::{
                MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey,
            },
            WindowsAndMessaging::{GetMessageW, MSG, PostThreadMessageW, WM_HOTKEY, WM_QUIT},
        },
    };

    /// Windows wants a number to tell this program's hotkeys apart. We hold
    /// exactly one.
    const HOTKEY_ID: i32 = 1;

    pub(super) fn take(combination: Combination, ctx: Context) -> Result<Taken, GlobalState> {
        let Some(key) = virtual_key(combination.key) else {
            return Err(GlobalState::Refused(format!(
                "{combination} is not a key Windows can be told"
            )));
        };
        let modifiers = modifier_flags(combination);

        let (ready_sender, ready) = crossbeam_channel::bounded(1);
        let (press_sender, presses) = crossbeam_channel::unbounded();
        // The message that stops the loop has to be posted to the thread that
        // owns the hotkey, so the thread reports its own id on the way up.
        let thread_id = Arc::new(AtomicU32::new(0));
        let reported_id = thread_id.clone();

        let thread = thread::Builder::new()
            .name("cla-global-hotkey".to_owned())
            .spawn(move || {
                reported_id.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
                // MOD_NOREPEAT is Windows' own answer to a held key: the
                // hotkey fires once and not again until it is let go.
                let registered =
                    unsafe { RegisterHotKey(ptr::null_mut(), HOTKEY_ID, modifiers, key) };
                if registered == 0 {
                    let _ = ready_sender.send(Err(GlobalState::Refused(format!(
                        "{combination} is already held by another program — pick a different one"
                    ))));
                    return;
                }
                let _ = ready_sender.send(Ok(()));

                let mut message: MSG = unsafe { std::mem::zeroed() };
                while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
                    if message.message == WM_HOTKEY {
                        if press_sender.send(()).is_err() {
                            break;
                        }
                        // The window is very likely behind a full-screen game
                        // and drawing nothing; without this the press waits in
                        // the channel until something else wakes it.
                        ctx.request_repaint();
                    }
                }
                unsafe { UnregisterHotKey(ptr::null_mut(), HOTKEY_ID) };
            })
            .map_err(|error| {
                GlobalState::Unsupported(format!("the thread could not be started ({error})"))
            })?;

        match ready.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Taken {
                presses,
                release: Box::new(move || {
                    let id = thread_id.load(Ordering::Relaxed);
                    if id != 0 {
                        unsafe { PostThreadMessageW(id, WM_QUIT, 0, 0) };
                    }
                    let _ = thread.join();
                }),
            }),
            Ok(Err(state)) => {
                let _ = thread.join();
                Err(state)
            }
            Err(_) => Err(GlobalState::Unsupported(
                "Windows did not answer the request".to_owned(),
            )),
        }
    }

    fn modifier_flags(combination: Combination) -> windows_sys::Win32::UI::Input::KeyboardAndMouse::HOT_KEY_MODIFIERS {
        let modifiers = combination.modifiers;
        let mut flags = MOD_NOREPEAT;
        if modifiers.shift {
            flags |= MOD_SHIFT;
        }
        if modifiers.ctrl || modifiers.command {
            flags |= MOD_CONTROL;
        }
        if modifiers.alt {
            flags |= MOD_ALT;
        }
        flags
    }

    /// The virtual-key code of a key, for the three kinds
    /// [`Combination::is_supported`] allows. Letters and digits are their ASCII
    /// value; `VK_F1` is 0x70 and the rest follow it.
    fn virtual_key(key: eframe::egui::Key) -> Option<u32> {
        let name = key.name();
        let mut characters = name.chars();
        match (characters.next(), characters.next()) {
            (Some(letter), None) if letter.is_ascii_uppercase() => Some(u32::from(letter as u8)),
            (Some(digit), None) if digit.is_ascii_digit() => Some(u32::from(digit as u8)),
            (Some('F'), Some(_)) => name[1..]
                .parse::<u32>()
                .ok()
                .filter(|number| (1..=24).contains(number))
                .map(|number| 0x70 + number - 1),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------ other

#[cfg(not(any(target_os = "linux", windows)))]
mod backend {
    use super::*;

    pub(super) fn take(_combination: Combination, _ctx: Context) -> Result<Taken, GlobalState> {
        Err(GlobalState::Unsupported(
            "this platform has no way to take a key from the desktop".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing is held until something is asked for, and the settings tab has a
    /// sentence to show either way.
    #[test]
    fn a_hotkey_that_was_never_asked_for_holds_nothing() {
        let hotkey = GlobalHotkey::off(Context::default());
        assert!(!hotkey.holds_a_key());
        assert_eq!(&GlobalState::Off, hotkey.state());
        assert!(!hotkey.state().is_problem());
        assert!(!hotkey.state().message().is_empty());
    }

    /// A refusal reads as a problem, so the tab can colour it — a key the
    /// reader asked for and did not get must not look like one that works.
    #[test]
    fn a_refusal_is_reported_as_a_problem() {
        assert!(GlobalState::Refused("taken".to_owned()).is_problem());
        assert!(GlobalState::Unsupported("no X server".to_owned()).is_problem());
        assert!(!GlobalState::Held("Alt+O".to_owned()).is_problem());
    }

    /// The whole of the Linux path against a real X server: the key is found on
    /// the layout, the grab is accepted, a press arrives, and the key is given
    /// back when the hotkey is dropped.
    ///
    /// Ignored by default because it needs an X server and `xdotool`, and
    /// because a grab is a change to whatever desktop it runs on. It is meant
    /// for a throwaway one:
    ///
    /// ```text
    /// Xvfb :99 &
    /// DISPLAY=:99 cargo test a_grab_catches_the_key -- --ignored --nocapture
    /// ```
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs an X server and xdotool"]
    fn a_grab_catches_the_key() {
        use eframe::egui::{Key, Modifiers};
        use std::process::Command;

        let combination = Combination {
            modifiers: Modifiers::ALT | Modifiers::CTRL,
            key: Key::Y,
        };

        let mut hotkey = GlobalHotkey::off(Context::default());
        hotkey.bind(Some(combination));
        assert_eq!(
            &GlobalState::Held(combination.to_string()),
            hotkey.state(),
            "the key was not taken: {}",
            hotkey.state().message()
        );

        // Pressed by anyone, anywhere on this display — which is the whole
        // point of the grab, and the reason this cannot be asserted without
        // one.
        let sent = Command::new("xdotool")
            .args(["key", "--clearmodifiers", "ctrl+alt+y"])
            .status()
            .expect("xdotool is needed for this test");
        assert!(sent.success());

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while hotkey.presses() == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the press never arrived"
            );
            thread::sleep(Duration::from_millis(20));
        }

        // Given back on drop: a second hotkey can only take the same key once
        // the first has let go, so this asserts the release as well.
        drop(hotkey);
        let mut again = GlobalHotkey::off(Context::default());
        again.bind(Some(combination));
        assert_eq!(
            &GlobalState::Held(combination.to_string()),
            again.state(),
            "the key was not given back: {}",
            again.state().message()
        );
    }
}
