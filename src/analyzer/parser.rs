use std::{
    collections::VecDeque,
    fmt::Write,
    fs::File,
    io::{BufReader, Seek},
    ops::Range,
    path::Path,
};

use chrono::NaiveDateTime;
use lazy_static::lazy_static;
use log::error;
use regex::Regex;

use super::*;

#[derive(Debug)]
pub struct Record<'a> {
    pub time: NaiveDateTime,
    pub source: Entity<'a>,
    pub target: Entity<'a>,
    pub indirect_source: Entity<'a>, // e.g. a pet
    pub value_name: Cow<'a, str>,
    pub value_type: Cow<'a, str>,
    pub value_flags: ValueFlags,
    pub value: RecordValue,
    pub _raw: &'a str,
    pub log_pos: Option<Range<u64>>,
}

#[derive(Debug)]
pub enum Entity<'a> {
    None,
    Player {
        full_name: Cow<'a, str>, // -> name@handle
        _id: (u64, u64),
    },
    NonPlayer {
        name: Cow<'a, str>,
        _id: u64,
        unique_name: Cow<'a, str>,
    },
    NonPlayerCharacter {
        _id: u64,
        name: Cow<'a, str>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum RecordValue {
    Damage(BaseHit),
    Heal(BaseHealTick),
}

pub struct Parser {
    line_parser: LineParser,
    scratch_pad: String,
    /// Complete lines read past the current one while resolving an ambiguous
    /// shield line, oldest first. Served before anything new is read.
    lookahead: VecDeque<PeekedLine>,
    /// Copy of the current line, used only when we had to read ahead — peeking
    /// overwrites the shared line buffer.
    current: PeekedLine,
    /// Whether the current record's text lives in `current` rather than in the
    /// line parser's buffer.
    current_is_owned: bool,
}

/// A fully read line together with its byte range, so lines held in the
/// lookahead keep the positions Save Combat and combat deletion rely on.
#[derive(Default)]
struct PeekedLine {
    text: String,
    start: Option<u64>,
    end: Option<u64>,
}

/// What a shield line and the damage line of the same shot have in common.
///
/// Deliberately **not** including the source field: the reference notes that a
/// shot's shield and hull lines may carry different sources (a pet credited on
/// one of them), and real logs do exactly that.
struct LineKey {
    timestamp: String,
    owner_id: String,
    target_id: String,
    ability: String,
}

pub enum RecordError<'a> {
    EndReached,
    InvalidRecord(&'a str),
}

impl Parser {
    pub fn new(file_name: &Path) -> Option<Self> {
        let file = File::options()
            .read(true)
            .write(false)
            .open(file_name)
            .ok()?;

        let file = BufReader::with_capacity(1 << 20, file); // 1MB

        Some(Self {
            line_parser: LineParser {
                char_parser: CharParser { file },
                escaped: Default::default(),
                line: Default::default(),
                // The first line starts at byte 0; advance_line only records the
                // start of later lines (after a newline), so seed it here.
                // Otherwise the first combat has no log position and can't be
                // read back (Save Combat, delete combats).
                line_start_in_file: Some(0),
                line_end_in_file: Default::default(),
                line_finished: Default::default(),
            },
            scratch_pad: String::new(),
            lookahead: VecDeque::new(),
            current: PeekedLine::default(),
            current_is_owned: false,
        })
    }

    pub fn parse_next(&mut self) -> Result<Record<'_>, RecordError<'_>> {
        // Lines read past the current one during an earlier lookahead are served
        // first, so nothing is parsed twice or skipped.
        self.current_is_owned = match self.lookahead.pop_front() {
            Some(peeked) => {
                self.current = peeked;
                true
            }
            None => {
                if !self.line_parser.advance_line() {
                    return Err(RecordError::EndReached);
                }
                false
            }
        };

        let log_pos = if self.current_is_owned {
            match (self.current.start, self.current.end) {
                (Some(s), Some(e)) => Some(s..e),
                _ => None,
            }
        } else {
            match (
                self.line_parser.line_start_in_file,
                self.line_parser.line_end_in_file,
            ) {
                (Some(s), Some(e)) => Some(s..e),
                _ => None,
            }
        };

        // A shield line whose base magnitude is zero is shaped exactly like a
        // shield heal, so the line alone cannot say which it is. Only the rest
        // of the shot can: an attack also writes a damage line at the same
        // timestamp, a heal does not.
        let shield_line_is_damage = if Self::is_ambiguous_shield_line(self.current_line()) {
            self.resolve_ambiguous_shield_line()
        } else {
            false
        };

        let line: &str = if self.current_is_owned {
            &self.current.text
        } else {
            &self.line_parser.line
        };
        Self::parse_from_line(line, &mut self.scratch_pad, log_pos, shield_line_is_damage)
            .ok_or(RecordError::InvalidRecord(line))
    }

    fn current_line(&self) -> &str {
        if self.current_is_owned {
            &self.current.text
        } else {
            &self.line_parser.line
        }
    }

    /// Reads ahead over the rest of the current timestamp looking for the
    /// damage line that belongs to the same shot. Returns whether one was
    /// found, i.e. whether the shield line in hand is an attack rather than a
    /// heal.
    ///
    /// Everything read is kept in `lookahead` and served by later calls, so the
    /// stream is neither rewound nor lost. When the log ends mid-group nothing
    /// is found and the line is treated as a heal; the remainder arrives on a
    /// later refresh, by which time this record is already recorded. That is a
    /// bounded inaccuracy at the very tail of a log that stops growing.
    fn resolve_ambiguous_shield_line(&mut self) -> bool {
        let Some(key) = LineKey::of(self.current_line()) else {
            return false;
        };

        // Take a copy: peeking writes through the shared line buffer.
        if !self.current_is_owned {
            self.current.text.clear();
            self.current.text.push_str(&self.line_parser.line);
            self.current.start = self.line_parser.line_start_in_file;
            self.current.end = self.line_parser.line_end_in_file;
            self.current_is_owned = true;
        }

        if self.lookahead.iter().any(|p| key.is_companion(&p.text)) {
            return true;
        }

        while self.line_parser.advance_line() {
            let peeked = PeekedLine {
                text: self.line_parser.line.clone(),
                start: self.line_parser.line_start_in_file,
                end: self.line_parser.line_end_in_file,
            };
            let same_shot_window = key.is_same_timestamp(&peeked.text);
            let is_companion = same_shot_window && key.is_companion(&peeked.text);
            self.lookahead.push_back(peeked);
            if is_companion {
                return true;
            }
            if !same_shot_window {
                break;
            }
        }

        false
    }

    /// Whether a raw line is a shield line that could be either damage or a
    /// heal: negative magnitude, zero base magnitude, no `ShieldBreak` (which
    /// already marks it as an attack). Cheap checks first — this runs on every
    /// line of the log.
    fn is_ambiguous_shield_line(line: &str) -> bool {
        if !line.contains(",Shield,") {
            return false;
        }
        let Some(last_comma) = line.rfind(',') else {
            return false;
        };
        if line[last_comma + 1..].trim().parse::<f64>() != Ok(0.0) {
            return false;
        }

        let Some(fields) = LineFields::of(line) else {
            return false;
        };
        fields.value_type == "Shield"
            && !fields.flags.contains("ShieldBreak")
            && fields.value1 < 0.0
            && fields.value2 == 0.0
    }

    fn parse_from_line<'a>(
        line: &'a str,
        scratch_pad: &mut String,
        log_pos: Option<Range<u64>>,
        shield_line_is_damage: bool,
    ) -> Option<Record<'a>> {
        let (time, line) = line.split_once("::")?;

        let time = Self::parse_time(time, scratch_pad)?;

        let mut fields = parse_csv_line(line);
        let source_name = fields.next()?;
        let source_id_and_unique_name = fields.next()?;
        let source = Entity::parse(source_name, source_id_and_unique_name)?;

        let indirect_source_name = fields.next()?;
        let indirect_source_id_and_unique_name = fields.next()?;
        let indirect_source =
            Entity::parse(indirect_source_name, indirect_source_id_and_unique_name)?;

        let target_name = fields.next()?;
        let target_id_and_unique_name = fields.next()?;
        let target = Entity::parse(target_name, target_id_and_unique_name)?;

        let value_name = fields.next()?;

        // don't know what these are (e.g. Pn.Rfd0cd)
        fields.next()?;

        let value_type = fields.next()?;
        let value_flags = fields.next()?;
        let value_flags = value_flags.trim();
        let value_flags = ValueFlags::parse(value_flags);
        let value1 = fields.next()?;
        let value2 = fields.next()?;

        let value = RecordValue::new(
            value_type.trim(),
            value1.trim(),
            value2.trim(),
            value_flags,
            shield_line_is_damage,
        )?;

        let record = Record {
            time,
            source,
            target,
            indirect_source,
            value_name,
            value_type,
            value_flags,
            value,
            _raw: line,
            log_pos,
        };
        Some(record)
    }

    fn parse_time<'b>(time: &'b str, scratch_pad: &mut String) -> Option<NaiveDateTime> {
        scratch_pad.clear();
        write!(scratch_pad, "{}00", time).ok()?;
        let time = NaiveDateTime::parse_from_str(&scratch_pad, "%y:%m:%d:%H:%M:%S%.3f").ok()?;

        Some(time)
    }
}

impl<'a> Record<'a> {
    pub fn is_player_out_damage(&self) -> bool {
        self.source.is_player() && self.value.is_damage()
    }

    pub fn is_immune_or_zero(&self) -> bool {
        self.value.is_all_zero() || self.value_flags.contains(ValueFlags::IMMUNE)
    }

    pub fn is_self_directed(&self) -> bool {
        self.target.is_none() && self.indirect_source.is_none()
    }

    pub fn is_direct_self_damage(&self) -> bool {
        self.is_self_directed() && self.value.is_damage()
    }
}

/// The handful of fields needed to compare two raw lines, without building a
/// whole [`Record`] (which would allocate and resolve entities).
struct LineFields<'a> {
    timestamp: &'a str,
    owner_id: Cow<'a, str>,
    target_id: Cow<'a, str>,
    ability: Cow<'a, str>,
    value_type: Cow<'a, str>,
    flags: Cow<'a, str>,
    value1: f64,
    value2: f64,
}

impl<'a> LineFields<'a> {
    fn of(line: &'a str) -> Option<Self> {
        let (timestamp, rest) = line.split_once("::")?;
        let mut fields = parse_csv_line(rest);
        let _owner_name = fields.next()?;
        let owner_id = fields.next()?;
        let _source_name = fields.next()?;
        let _source_id = fields.next()?;
        let _target_name = fields.next()?;
        let target_id = fields.next()?;
        let ability = fields.next()?;
        let _internal_ability = fields.next()?;
        let value_type = fields.next()?;
        let flags = fields.next()?;
        let value1 = fields.next()?.trim().parse::<f64>().ok()?;
        let value2 = fields.next()?.trim().parse::<f64>().ok()?;
        Some(Self {
            timestamp,
            owner_id,
            target_id,
            ability,
            value_type,
            flags,
            value1,
            value2,
        })
    }
}

impl LineKey {
    fn of(line: &str) -> Option<Self> {
        let fields = LineFields::of(line)?;
        Some(Self {
            timestamp: fields.timestamp.to_string(),
            owner_id: fields.owner_id.into_owned(),
            target_id: fields.target_id.into_owned(),
            ability: fields.ability.into_owned(),
        })
    }

    fn is_same_timestamp(&self, line: &str) -> bool {
        line.split_once("::")
            .map(|(timestamp, _)| timestamp == self.timestamp)
            .unwrap_or(false)
    }

    /// Whether `line` is the damage half of the same shot: same instant, owner,
    /// target and ability, and an actual damage line — a negative `HitPoints`
    /// line is another heal (abilities that restore hull and shields at once
    /// write both), not the companion we are looking for.
    fn is_companion(&self, line: &str) -> bool {
        let Some(fields) = LineFields::of(line) else {
            return false;
        };
        fields.timestamp == self.timestamp
            && fields.owner_id == self.owner_id
            && fields.target_id == self.target_id
            && fields.ability == self.ability
            && fields.value_type != "Shield"
            && !(fields.value_type == "HitPoints" && fields.value1 < 0.0)
    }
}

lazy_static! {
    static ref ID_AND_UNIQUE_NAME_REGEX: Regex = Regex::new(
        r"(?P<type>P|C|S)\[(?P<id>\d+)(@(?P<player_id>\d+))?(\s+(?P<unique_name>[^\]]+))?\]"
    )
    .unwrap();
}
impl<'a> Entity<'a> {
    fn parse(name: Cow<'a, str>, id_and_unique_name: Cow<'a, str>) -> Option<Self> {
        if name.is_empty() && (id_and_unique_name.is_empty() || id_and_unique_name == "*") {
            return Some(Self::None);
        }

        let captures = ID_AND_UNIQUE_NAME_REGEX.captures(&id_and_unique_name)?;
        let entity_type = captures.name("type")?.as_str();
        let id = captures.name("id")?.as_str();
        let id = str::parse::<u64>(id).ok()?;

        fn map_cow_substr(cow: Cow<str>, range: Range<usize>) -> Cow<str> {
            match cow {
                Cow::Borrowed(s) => Cow::Borrowed(&s[range]),
                Cow::Owned(s) => Cow::Owned(s[range].to_string()),
            }
        }

        match entity_type {
            "P" => {
                let player_id = captures.name("player_id")?.as_str();
                let player_id = str::parse::<u64>(player_id).ok()?;
                let unique_name = captures.name("unique_name")?.range();

                Some(Self::Player {
                    full_name: map_cow_substr(id_and_unique_name, unique_name),
                    _id: (id, player_id),
                })
            }
            "C" => {
                let unique_name = captures.name("unique_name")?.range();
                Some(Self::NonPlayer {
                    name,
                    _id: id,
                    unique_name: map_cow_substr(id_and_unique_name, unique_name),
                })
            }
            "S" => Some(Self::NonPlayerCharacter { _id: id, name }),
            _ => None,
        }
    }

    pub fn name(&self) -> Option<&str> {
        match self {
            Entity::None => None,
            Entity::Player { full_name, .. } => Some(full_name),
            Entity::NonPlayer { name, .. } => Some(name),
            Entity::NonPlayerCharacter { name, .. } => Some(name),
        }
    }

    pub fn unique_name(&self) -> Option<&str> {
        match self {
            Entity::None => None,
            Entity::Player { full_name, .. } => Some(full_name),
            Entity::NonPlayer { unique_name, .. } => Some(unique_name),
            Entity::NonPlayerCharacter { .. } => None,
        }
    }

    pub fn is_player(&self) -> bool {
        match self {
            Entity::Player { .. } => true,
            _ => false,
        }
    }

    pub fn is_none(&self) -> bool {
        match self {
            Entity::None { .. } => true,
            _ => false,
        }
    }
}

impl RecordValue {
    /// `shield_line_is_damage` resolves the one case the numbers cannot: a
    /// shield line with a negative magnitude and a zero base magnitude is
    /// shaped identically whether it restored a shield or an attack took one
    /// down. The parser decides it by looking for the shot's damage line and
    /// passes the answer in here.
    pub fn new(
        value_type: &str,
        value1: &str,
        value2: &str,
        flags: ValueFlags,
        shield_line_is_damage: bool,
    ) -> Option<Self> {
        let value1 = str::parse::<f64>(value1).ok()?;
        let value2 = str::parse::<f64>(value2).ok()?;

        // A negative HitPoints value is healing; positive hull damage falls
        // through to the generic hull branch at the bottom.
        if value1 < 0.0 && value_type == "HitPoints" {
            return Some(Self::Heal(BaseHealTick::hull(value1, flags)));
        }

        if value_type == "Shield" {
            if value2 == 0.0 && !flags.contains(ValueFlags::SHIELD_BREAK) {
                if value1 < 0.0 {
                    if shield_line_is_damage {
                        // Shield damage that happens to record nothing kept off
                        // the hull. Its "prevented" half is genuinely zero.
                        return Some(Self::Damage(BaseHit::shield(value1, flags, 0.0)));
                    }
                    return Some(Self::Heal(BaseHealTick::shield(value1, flags)));
                }

                if value1 > 0.0 {
                    return Some(Self::Damage(BaseHit::shield_drain(value1, flags)));
                }
            }
            return Some(Self::Damage(BaseHit::shield(value1, flags, value2)));
        }

        if value2 == 0.0 {
            return Some(Self::Damage(BaseHit::hull(value1, flags, value1)));
        }
        return Some(Self::Damage(BaseHit::hull(value1, flags, value2)));
    }

    pub fn is_all_zero(&self) -> bool {
        match self {
            RecordValue::Damage(v) => {
                v.damage == 0.0
                    && match v.specific {
                        SpecificHit::Shield {
                            damage_prevented_to_hull,
                        } => damage_prevented_to_hull == 0.0,
                        SpecificHit::ShieldDrain => true,
                        SpecificHit::Hull { base_damage } => base_damage == 0.0,
                    }
            }
            RecordValue::Heal(v) => v.amount == 0.0,
        }
    }

    pub fn is_damage(&self) -> bool {
        match self {
            RecordValue::Damage(_) => true,
            RecordValue::Heal(_) => false,
        }
    }
}

impl<'a> From<std::io::Error> for RecordError<'a> {
    fn from(_: std::io::Error) -> Self {
        RecordError::EndReached
    }
}

struct LineParser {
    char_parser: CharParser,
    escaped: bool,
    line: String,
    line_start_in_file: Option<u64>,
    line_end_in_file: Option<u64>,
    line_finished: bool,
}

impl LineParser {
    fn advance_line(&mut self) -> bool {
        if self.line_finished {
            self.line.clear();
            self.line_finished = false;
            self.line_start_in_file = self.char_parser.pos();
        }
        loop {
            let Some(c) = self.char_parser.next() else {
                return false;
            };
            if c == '\n' && !self.escaped {
                self.line_end_in_file = self.char_parser.pos();
                self.line_finished = true;
                return true;
            }

            self.line.push(c);

            if c == '"' {
                self.escaped = !self.escaped;
            }
        }
    }
}

fn parse_csv_line<'a>(line: &'a str) -> impl Iterator<Item = Cow<'a, str>> {
    let mut chars = line.char_indices();
    std::iter::from_fn(move || {
        let (start_index, first_c) = chars.next()?;
        if first_c == ',' {
            return Some(Cow::Borrowed(""));
        }
        let is_escaped = first_c == '"';
        if is_escaped {
            let start_index = start_index + 1;
            let mut field = Cow::Borrowed("");
            loop {
                let Some((index, c)) = chars.next() else {
                    return Some(Cow::Borrowed(&line[start_index..]));
                };

                if c == '"' {
                    let Some((_, c)) = chars.next() else {
                        return Some(Cow::Borrowed(&line[start_index..]));
                    };

                    match c {
                        ',' => return Some(field),     // field end
                        '"' => field.to_mut().push(c), // escaped quote
                        _ => {
                            error!("record CSV syntax error: {}", line);
                            return None;
                        }
                    }
                    if c != '"' {
                        return Some(field);
                    }

                    field.to_mut().push(c);
                } else {
                    match &mut field {
                        Cow::Borrowed(field) => *field = &line[start_index..=index],
                        Cow::Owned(field) => field.push(c),
                    }
                }
            }
        } else {
            loop {
                let Some((index, c)) = chars.next() else {
                    return Some(Cow::Borrowed(&line[start_index..]));
                };

                if c == ',' {
                    return Some(Cow::Borrowed(&line[start_index..index]));
                }
            }
        }
    })
}

struct CharParser {
    file: BufReader<File>,
}

/// Number of bytes in the UTF-8 sequence started by `lead`. A stray
/// continuation byte (0x80..=0xBF) reports 1 so `str::from_utf8` rejects it.
fn utf8_char_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xF0..=0xF7 => 4,
        0xE0..=0xEF => 3,
        0xC0..=0xDF => 2,
        _ => 1,
    }
}

impl CharParser {
    fn next(&mut self) -> Option<char> {
        let mut bytes = [0u8; 4];
        if self.file.read(std::slice::from_mut(&mut bytes[0])).ok()? == 0 {
            return None;
        }
        let len = utf8_char_len(bytes[0]);
        for byte in bytes.iter_mut().take(len).skip(1) {
            if self.file.read(std::slice::from_mut(byte)).ok()? == 0 {
                error!("truncated UTF-8 sequence at end of log");
                return None;
            }
        }
        match std::str::from_utf8(&bytes[..len]) {
            Ok(s) => s.chars().next(),
            Err(_) => {
                error!("invalid UTF-8 sequence in log: {:x?}", &bytes[..len]);
                None
            }
        }
    }

    fn pos(&mut self) -> Option<u64> {
        self.file.stream_position().ok()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[ignore = "manual test"]
    #[test]
    fn read_log() {
        let mut parser =
            Parser::new(&PathBuf::from(r"/home/nathan/Downloads/combatlog.log")).unwrap();

        let mut record_data = Vec::new();
        loop {
            match parser.parse_next() {
                Ok(record) => record_data.push(record.time),
                Err(RecordError::InvalidRecord(invalid_record)) => {
                    panic!("{}", invalid_record);
                }
                Err(RecordError::EndReached) => break,
            };
        }

        // println!("{:?}", record_data);
    }

    #[ignore = "manual test"]
    #[test]
    fn single_record() {
        let record = Parser::parse_from_line(
            "23:01:07:10:12:56.3::Borg Queen Octahedron,C[25 Mission_Space_Borg_Queen_Diamond],Ayel,P[12793028@5473940 Ayel@greyblizzard],,*,Plasma Fire,Pn.Wujkxq,Plasma,Kill,2086.87,5300.66",
            &mut String::new(),
            None,
            false)
            .unwrap();

        println!("{:?}", record)
    }

    /// Parses `lines` and reports, per record, whether it came out as damage or
    /// as a heal, with the magnitude and the byte range it claims in the file.
    fn parse_all(name: &str, lines: &str) -> Vec<(bool, f64, Option<Range<u64>>)> {
        let path = std::env::temp_dir().join(format!("cla-parser-{name}.log"));
        std::fs::write(&path, lines).unwrap();
        let mut parser = Parser::new(&path).unwrap();
        let mut out = Vec::new();
        loop {
            match parser.parse_next() {
                Ok(record) => out.push(match record.value {
                    RecordValue::Damage(hit) => (true, hit.damage, record.log_pos.clone()),
                    RecordValue::Heal(tick) => (false, tick.amount, record.log_pos.clone()),
                }),
                Err(RecordError::EndReached) => break,
                Err(RecordError::InvalidRecord(line)) => panic!("invalid record: {line}"),
            }
        }
        std::fs::remove_file(&path).ok();
        out
    }

    /// The shield half of an attack can record a zero base magnitude, which
    /// makes it identical in shape to a shield heal. The shot's damage line, at
    /// the same instant and on the same target, is what tells them apart.
    /// Taken from a real log: Chain Conduit Capacitor on an Elite Tactical
    /// Assimilated Gorn.
    #[test]
    fn a_shield_line_with_a_damage_line_beside_it_is_damage() {
        let records = parse_all(
            "shield-attack",
            concat!(
                "26:07:30:12:09:07.8::Kestrel,P[1@2 Kestrel@h],,*,Gorn,C[1758 Ground_Borg_Capt_Gorn],Chain Conduit Capacitor,Pn.M,Shield,,-856.734,0\n",
                "26:07:30:12:09:07.8::Kestrel,P[1@2 Kestrel@h],,*,Gorn,C[1758 Ground_Borg_Capt_Gorn],Chain Conduit Capacitor,Pn.M,Electrical,,214.184,1070.92\n",
            ),
        );
        assert_eq!(2, records.len());
        assert!(records[0].0, "the shield line must be damage, not a heal");
        assert_eq!(856.734, records[0].1);
        assert!(records[1].0);
    }

    /// The same shape with nothing else at that instant is a genuine shield
    /// heal — a self-directed proc, here Reflexive Emitters.
    #[test]
    fn a_lone_shield_line_is_a_heal() {
        let records = parse_all(
            "shield-heal",
            "26:07:24:19:54:57.0::Kestrel,P[1@2 Kestrel@h],,*,,*,Reflexive Emitters,Pn.D,Shield,,-2000,0\n",
        );
        assert_eq!(1, records.len());
        assert!(!records[0].0, "a lone shield line is a heal");
        assert_eq!(2000.0, records[0].1);
    }

    /// Abilities that restore hull and shields in the same instant write a
    /// negative `HitPoints` line beside the shield line. That is a second heal,
    /// not the damage line of a shot, and must not flip the shield line.
    /// Real case: Mudd's Time Bracelet.
    #[test]
    fn a_hull_heal_beside_a_shield_heal_keeps_both_heals() {
        let records = parse_all(
            "heal-pair",
            concat!(
                "26:07:26:12:59:32.5::Kestrel,P[1@2 Kestrel@h],,*,,*,Mudd's Time Bracelet,Pn.R,HitPoints,,-1099.16,0\n",
                "26:07:26:12:59:32.5::Kestrel,P[1@2 Kestrel@h],,*,,*,Mudd's Time Bracelet,Pn.R,Shield,,-274.79,0\n",
            ),
        );
        assert_eq!(2, records.len());
        assert!(!records[0].0, "the hull line is a heal");
        assert!(!records[1].0, "the shield line stays a heal");
    }

    /// A damage line for a *different* target at the same instant is not the
    /// companion, so the shield heal stays a heal.
    #[test]
    fn a_damage_line_on_another_target_is_not_the_companion() {
        let records = parse_all(
            "other-target",
            concat!(
                "26:07:30:12:09:07.8::Kestrel,P[1@2 Kestrel@h],,*,,*,Reflexive Emitters,Pn.D,Shield,,-2000,0\n",
                "26:07:30:12:09:07.8::Kestrel,P[1@2 Kestrel@h],,*,Gorn,C[1758 G],Chain Conduit Capacitor,Pn.M,Electrical,,214.184,1070.92\n",
            ),
        );
        assert!(!records[0].0, "an unrelated attack must not flip the heal");
        assert!(records[1].0);
    }

    /// A shield line that already carries a base magnitude was never ambiguous
    /// and needs no lookahead.
    #[test]
    fn a_shield_line_with_a_base_magnitude_is_damage_on_its_own() {
        let records = parse_all(
            "shield-with-base",
            "26:07:30:12:08:52.8::Kestrel,P[1@2 Kestrel@h],,*,Queen,C[48 G],Chain Conduit Capacitor,Pn.M,Shield,,-994.537,-1747.87\n",
        );
        assert!(records[0].0);
        assert_eq!(994.537, records[0].1);
    }

    /// Reading ahead must not disturb the byte ranges: Save Combat and combat
    /// deletion slice the log with them, so every record's range has to stay
    /// exact and contiguous even across a lookahead.
    #[test]
    fn byte_ranges_survive_a_lookahead() {
        let lines = concat!(
            "26:07:30:12:09:07.8::Kestrel,P[1@2 Kestrel@h],,*,Gorn,C[1758 G],Chain Conduit Capacitor,Pn.M,Shield,,-856.734,0\n",
            "26:07:30:12:09:07.8::Kestrel,P[1@2 Kestrel@h],,*,Gorn,C[1758 G],Chain Conduit Capacitor,Pn.M,Electrical,,214.184,1070.92\n",
            "26:07:30:12:09:09.0::Kestrel,P[1@2 Kestrel@h],,*,Gorn,C[1758 G],Phaser Beam,Pn.P,Phaser,,10,10\n",
        );
        let records = parse_all("byte-ranges", lines);
        assert_eq!(3, records.len());

        let mut expected_start = 0u64;
        for (index, (_, _, log_pos)) in records.iter().enumerate() {
            let range = log_pos.clone().expect("every record has a byte range");
            assert_eq!(
                expected_start, range.start,
                "record {index} must start where the previous one ended"
            );
            expected_start = range.end;
        }
        assert_eq!(
            lines.len() as u64,
            expected_start,
            "the ranges together must cover the whole file"
        );
    }

    #[test]
    fn decodes_utf8_multibyte() {
        // Covers 1-, 2-, 3- and 4-byte UTF-8 sequences: 'a', 'é', '—', '🚀'.
        let text = "aé—🚀";
        let path = std::env::temp_dir().join("cla_utf8_decode_test.log");
        std::fs::write(&path, text.as_bytes()).unwrap();

        let mut parser = CharParser {
            file: BufReader::new(File::open(&path).unwrap()),
        };
        let mut decoded = String::new();
        while let Some(c) = parser.next() {
            decoded.push(c);
        }
        std::fs::remove_file(&path).ok();

        assert_eq!(decoded, text);
    }
}
