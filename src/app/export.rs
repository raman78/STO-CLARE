//! Writing a comparison out as a spreadsheet.
//!
//! The sheet is the table as it stands on screen, minus the differences: plain
//! numbers a spreadsheet can add up, sort and chart on its own. The deltas are
//! left out on purpose — they are one combat measured against another, which a
//! spreadsheet can work out for itself from the columns beside it, and which
//! would otherwise arrive as text nothing can compute with.

use std::path::Path;

use rust_xlsxwriter::{Format, FormatAlign, Workbook, XlsxError};

/// The number formats a column can be written in, indexed by how many decimals
/// the metric is shown with on screen.
const NUMBER_FORMATS: [&str; 3] = ["#,##0", "#,##0.0", "#,##0.00"];

/// A whole sheet: which combats it is about, what the columns are, and the
/// rows of the ability tree.
pub struct Sheet {
    /// What the sheet is called in the workbook — the tab it came from, or
    /// "Comparison".
    pub name: String,
    pub combats: Vec<Combat>,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
}

/// One of the combats the comparison is of, named as the program names it.
pub struct Combat {
    pub identifier: String,
    pub note: String,
    pub player: String,
}

pub struct Column {
    pub header: String,
    /// Decimals the metric is shown with, so the spreadsheet rounds the way the
    /// table does rather than showing a DPS to fifteen places.
    pub decimals: usize,
}

pub struct Row {
    pub name: String,
    /// Depth in the ability tree, drawn as an indent so the file reads like the
    /// table it came from.
    pub level: usize,
    /// One entry per column; `None` where that column has nothing for this row
    /// and the cell is left empty rather than filled with a zero nobody meant.
    pub values: Vec<Option<f64>>,
}

/// The name the save dialog opens with: the combat the comparison starts from,
/// which is what the user picked first and is likeliest to recognize.
pub fn default_file_name(combat: &crate::analyzer::Combat) -> String {
    format!("{}.xlsx", combat.file_identifier())
}

/// Writes `sheets` to `path`, one worksheet each. Everything about the layout
/// lives here, so the table on screen and the file it produces can drift apart
/// only on purpose.
pub fn write(path: &Path, sheets: &[Sheet]) -> Result<(), XlsxError> {
    let mut workbook = Workbook::new();
    for sheet in sheets {
        write_sheet(&mut workbook, sheet)?;
    }
    workbook.save(path)?;
    Ok(())
}

fn write_sheet(workbook: &mut Workbook, sheet: &Sheet) -> Result<(), XlsxError> {
    let bold = Format::new().set_bold();
    let header = Format::new().set_bold().set_align(FormatAlign::Right);
    let numbers: Vec<Format> = sheet
        .columns
        .iter()
        .map(|column| {
            Format::new()
                .set_num_format(NUMBER_FORMATS[column.decimals.min(NUMBER_FORMATS.len() - 1)])
        })
        .collect();

    let sheet_data = workbook.add_worksheet();
    sheet_data.set_name(&sheet.name)?;

    // Which combat each "#n" is, so the columns can be read without the app.
    let mut row = 0u32;
    sheet_data.write_string_with_format(row, 0, "Combats compared", &bold)?;
    row += 1;
    for (index, combat) in sheet.combats.iter().enumerate() {
        sheet_data.write_string(row, 0, format!("#{}", index + 1))?;
        sheet_data.write_string(row, 1, &combat.identifier)?;
        sheet_data.write_string(row, 2, &combat.player)?;
        sheet_data.write_string(row, 3, &combat.note)?;
        row += 1;
    }
    row += 1;

    let header_row = row;
    sheet_data.write_string_with_format(header_row, 0, "Name", &bold)?;
    for (index, column) in sheet.columns.iter().enumerate() {
        sheet_data.write_string_with_format(
            header_row,
            column_at(index),
            &column.header,
            &header,
        )?;
    }
    row += 1;

    for entry in sheet.rows.iter() {
        sheet_data.write_string(row, 0, indented(&entry.name, entry.level))?;
        for (index, value) in entry.values.iter().enumerate() {
            if let Some(value) = value {
                sheet_data.write_number_with_format(
                    row,
                    column_at(index),
                    *value,
                    &numbers[index.min(numbers.len() - 1)],
                )?;
            }
        }
        row += 1;
    }

    // The names are long and the numbers are not, and a scrolled-away header
    // row is what makes a wide sheet unreadable.
    sheet_data.set_column_width(0, 44)?;
    sheet_data.set_freeze_panes(header_row + 1, 1)?;
    Ok(())
}

/// Where a column's values go: the first column is the ability name.
fn column_at(index: usize) -> u16 {
    index as u16 + 1
}

/// A row's name at its depth in the tree. Spaces rather than a cell indent, so
/// the shape survives a copy into anything else.
fn indented(name: &str, level: usize) -> String {
    format!("{}{name}", "    ".repeat(level))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet() -> Sheet {
        Sheet {
            name: "Comparison".to_string(),
            combats: vec![Combat {
                identifier: "Infected Space [Elite] | 2026-07-23 20:07:22 - 20:11:37".to_string(),
                note: "Cheops build".to_string(),
                player: "Kestrel".to_string(),
            }],
            columns: vec![
                Column {
                    header: "DPS #1".to_string(),
                    decimals: 0,
                },
                Column {
                    header: "Critical % #1".to_string(),
                    decimals: 2,
                },
            ],
            rows: vec![
                Row {
                    name: "Total".to_string(),
                    level: 0,
                    values: vec![Some(85231.0), Some(41.2)],
                },
                Row {
                    name: "Phaser Beam".to_string(),
                    level: 1,
                    values: vec![Some(21004.0), None],
                },
            ],
        }
    }

    /// The tree's shape is carried by indenting the name, which is all a
    /// spreadsheet has to show it with.
    #[test]
    fn a_sub_ability_is_indented_under_its_parent() {
        assert_eq!("Total", indented("Total", 0));
        assert_eq!("    Phaser Beam", indented("Phaser Beam", 1));
        assert_eq!("        Overload", indented("Overload", 2));
    }

    /// The first column is the name, so the values start beside it.
    #[test]
    fn values_start_after_the_name_column() {
        assert_eq!(1, column_at(0));
        assert_eq!(2, column_at(1));
    }

    /// The metric's own precision picks the format, and anything beyond the
    /// formats we have falls back to the last rather than panicking on an
    /// index — a metric shown with more decimals would otherwise take the
    /// export down with it.
    #[test]
    fn every_precision_has_a_number_format() {
        for format in &NUMBER_FORMATS {
            assert!(!format.is_empty());
        }
        assert_eq!(
            "#,##0.00",
            NUMBER_FORMATS[9usize.min(NUMBER_FORMATS.len() - 1)]
        );
    }

    /// A workbook holds a sheet per tab, each under its own name — Excel
    /// rejects a duplicate, so the names the tabs are called by have to be
    /// distinct and short enough (31 characters) to be sheet names at all.
    #[test]
    fn every_tab_can_be_a_sheet_of_one_workbook() {
        let dir = std::env::temp_dir().join(format!("cla-sheets-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("combat.xlsx");

        let names = [
            "Summary",
            "Damage Dealt",
            "Damage Taken",
            "Self Healing",
            "Healing Ally",
            "Healing Received",
        ];
        let sheets: Vec<Sheet> = names
            .iter()
            .map(|name| Sheet {
                name: name.to_string(),
                ..sheet()
            })
            .collect();

        write(&path, &sheets).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > 1000);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole point: a real file lands on disk and is a readable workbook
    /// (a zip archive, which is what xlsx is).
    #[test]
    fn writing_produces_a_workbook_on_disk() {
        let dir = std::env::temp_dir().join(format!("cla-export-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("comparison.xlsx");

        write(&path, std::slice::from_ref(&sheet())).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() > 1000, "an empty file is not a workbook");
        assert_eq!(
            b"PK",
            &bytes[..2],
            "an xlsx is a zip archive and starts with its signature"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
