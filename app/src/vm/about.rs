//! The About screen's data: the patch list (from `patches::about_list`) and the licences. The
//! licence rows follow ROADMAP.md's "Open-source pieces and where they fit" table; a test
//! keeps the two in step. The `.slint` file draws them; the texts are translated here because
//! Rust builds them (`lang::Texts`).

use crate::lang::Texts;
use uncharted_core::patches::{self, Patch, Selection, Target};

/// One open-source piece credited on About.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Licence {
    /// As the ROADMAP licence table names it.
    pub name: &'static str,
    /// The licence as that table gives it; "" when none is established.
    pub licence: &'static str,
    /// What it is for here, in English (translated through the catalogues).
    pub role: &'static str,
}

/// Code that ships in the launcher, its engine or the game runtime. Uncharted Universal itself
/// is GPL-2.0-only (the About screen says so above this list).
pub const SHIPPED: &[Licence] = &[
    Licence {
        name: "shadPS4",
        licence: "GPL-2.0",
        role: "Graphics engine (our fork, in gpu/)",
    },
    Licence {
        name: "Supermedo/uncharted_pc, spacepepperoni/uncharted_Linux",
        licence: "GPL-2.0",
        role: "The C and C++ port this one grew from",
    },
    Licence {
        name: "zorde23/Uncharted_Collection_PC",
        licence: "GPL-2.0",
        role: "Targeted fixes for bugs with the same cause",
    },
    Licence {
        name: "slint-ui/slint",
        licence: "Royalty-free",
        role: "The launcher's interface",
    },
    Licence {
        name: "SDL3",
        licence: "zlib",
        role: "Input, window and audio in the game",
    },
    Licence {
        name: "gilrs",
        licence: "MIT or Apache-2.0",
        role: "Controller input in the launcher",
    },
    Licence {
        name: "roxmltree",
        licence: "MIT or Apache-2.0",
        role: "Reading patch files",
    },
    Licence {
        name: "serde_json",
        licence: "MIT or Apache-2.0",
        role: "Settings and record files",
    },
    Licence {
        name: "sys-locale",
        licence: "MIT or Apache-2.0",
        role: "Finding the system language",
    },
    Licence {
        name: "iannitram/totk-native",
        licence: "MIT",
        role: "Platform, Steam, bundle and save card modules, copied with credit",
    },
];

/// Projects studied for how they work, with no code taken from them: every row of the ROADMAP
/// licence table whose use is "learn".
pub const LEARNED_FROM: &[Licence] = &[
    Licence {
        name: "bloodborne-native/bloodborne-launcher",
        licence: "GPL-2.0",
        role: "Launcher profiles and settings",
    },
    Licence {
        name: "Revivedx/SavageWindRecomp",
        licence: "MIT",
        role: "Release packaging",
    },
    Licence {
        name: "eladharels/uncharted_android",
        licence: "GPL-2.0",
        role: "Android device notes",
    },
    Licence {
        name: "Ryxm-a/uncharted-ios",
        licence: "GPL-2.0",
        role: "The iOS app's layout",
    },
    Licence {
        name: "truwa/nathan-drake-collection-macos",
        licence: "",
        role: "Mac packaging",
    },
];

/// One licence row as the screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenceRow {
    pub name: String,
    pub licence: String,
    pub role: String,
}

pub fn licence_rows(list: &[Licence], texts: &Texts) -> Vec<LicenceRow> {
    list.iter()
        .map(|item| LicenceRow {
            name: item.name.to_owned(),
            licence: if item.licence.is_empty() {
                texts.tr("No licence stated")
            } else {
                item.licence.to_owned()
            },
            role: texts.tr(item.role),
        })
        .collect()
}

/// One patch as the screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchRow {
    pub name: String,
    /// "by <author>", or "" when the file names none.
    pub author: String,
    /// "On", "Off", or "Not available: <why>".
    pub status: String,
    pub available: bool,
}

/// The two reasons `patches::about_list` gives itself; other reasons come from the patch file.
const OTHER_BUILD: &str = "This patch is for a different title, executable or revision.";
const NO_WRITES: &str = "This patch has no supported literal writes.";

/// Every patch for the chosen game, applicable or not (`patches::about_list`).
pub fn patch_rows(
    found: &[Patch],
    target: &Target,
    selection: &Selection,
    texts: &Texts,
) -> Vec<PatchRow> {
    patches::about_list(found, target, selection)
        .into_iter()
        .map(|entry| {
            let status = if !entry.available {
                let why = match entry.why_not.as_str() {
                    OTHER_BUILD => {
                        texts.tr("This patch is for a different title, executable or revision.")
                    }
                    NO_WRITES => texts.tr("This patch has no supported literal writes."),
                    from_the_file => from_the_file.to_owned(),
                };
                texts.tr1("Not available: {}", &why)
            } else if entry.enabled {
                texts.tr("On")
            } else {
                texts.tr("Off")
            };
            PatchRow {
                name: entry.name,
                author: if entry.author.is_empty() {
                    String::new()
                } else {
                    texts.tr1("by {}", &entry.author)
                },
                status,
                available: entry.available,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uncharted_core::language::Language;
    use uncharted_core::patches::ByteWrite;

    fn patch(name: &str, app_ver: &str, writes: usize, note: &str) -> Patch {
        Patch {
            key: format!("community.xml/{name}"),
            name: name.to_owned(),
            author: "A & B".to_owned(),
            note: note.to_owned(),
            title_ids: vec!["CUSA02320".to_owned()],
            executable: "eboot.bin".to_owned(),
            app_ver: app_ver.to_owned(),
            enabled_by_default: true,
            writes: (0..writes)
                .map(|i| ByteWrite {
                    offset: i as u64,
                    bytes: vec![0x90],
                })
                .collect(),
        }
    }

    fn target() -> Target {
        Target {
            title_id: "CUSA02320".to_owned(),
            executable: "eboot.bin".to_owned(),
            app_ver: "01.02".to_owned(),
        }
    }

    /// The rows of ROADMAP.md's licence table, as (name, licence, use) cells.
    fn roadmap_rows() -> Vec<(String, String, String)> {
        let roadmap = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ROADMAP.md"),
        )
        .unwrap();
        let section = roadmap
            .split("## Open-source pieces and where they fit")
            .nth(1)
            .expect("ROADMAP.md has the licence table")
            .split("\n## ")
            .next()
            .unwrap();
        section
            .lines()
            .filter(|line| line.starts_with("| ") && !line.starts_with("| Project"))
            .map(|line| {
                let cells: Vec<_> = line
                    .split(" | ")
                    .map(|c| c.trim_matches(['|', ' ']))
                    .collect();
                (
                    cells[0].to_owned(),
                    cells[1].to_owned(),
                    cells[3].to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn every_licence_row_is_complete_and_in_the_roadmap_table() {
        let table = roadmap_rows();
        assert!(table.len() > 20, "read {} ROADMAP rows", table.len());
        for item in SHIPPED.iter().chain(LEARNED_FROM) {
            assert!(!item.name.is_empty() && !item.role.is_empty(), "{item:?}");
            let row = table
                .iter()
                .find(|(name, _, _)| name.contains(item.name))
                .unwrap_or_else(|| panic!("{} is not in the ROADMAP licence table", item.name));
            assert!(
                row.1.to_lowercase().contains(&item.licence.to_lowercase()),
                "{}: About says {:?}, ROADMAP says {:?}",
                item.name,
                item.licence,
                row.1
            );
        }
    }

    #[test]
    fn every_learn_only_project_is_credited() {
        let learned: Vec<_> = roadmap_rows()
            .into_iter()
            .filter(|(_, _, used)| used.starts_with("learn"))
            .map(|(name, _, _)| name)
            .collect();
        assert!(!learned.is_empty());
        for name in &learned {
            assert!(
                LEARNED_FROM.iter().any(|item| name.contains(item.name)),
                "{name} is learn-only in ROADMAP.md but not credited on About"
            );
        }
        assert_eq!(learned.len(), LEARNED_FROM.len());
    }

    #[test]
    fn shadps4_slint_and_the_gpl_lineage_are_credited() {
        let shipped = |name: &str| SHIPPED.iter().find(|item| item.name == name).unwrap();
        assert_eq!(shipped("shadPS4").licence, "GPL-2.0");
        assert_eq!(shipped("slint-ui/slint").licence, "Royalty-free");
        assert!(
            shipped("Supermedo/uncharted_pc, spacepepperoni/uncharted_Linux")
                .licence
                .starts_with("GPL-2.0")
        );
        // Each dependency of the app's Cargo.toml is credited.
        let manifest =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
        for dependency in ["slint", "gilrs", "sys-locale"] {
            assert!(
                manifest.contains(&format!("\n{dependency} = ")),
                "{dependency}"
            );
        }
        for name in ["slint-ui/slint", "gilrs", "sys-locale"] {
            shipped(name);
        }
    }

    #[test]
    fn an_unstated_licence_is_said_in_words() {
        let rows = licence_rows(LEARNED_FROM, &Texts::new(Language::English));
        let truwa = rows
            .iter()
            .find(|row| row.name == "truwa/nathan-drake-collection-macos")
            .unwrap();
        assert_eq!(truwa.licence, "No licence stated");
        let rows = licence_rows(SHIPPED, &Texts::new(Language::French));
        assert_eq!(rows[0].licence, "GPL-2.0");
        assert_ne!(rows[0].role, SHIPPED[0].role);
    }

    #[test]
    fn patch_rows_say_on_off_and_why_not() {
        let found = [
            patch("Sixty", "01.02", 2, ""),
            patch("Other build", "01.00", 1, ""),
            patch(
                "Mask",
                "01.02",
                0,
                "(Unavailable: pattern patches are not supported)",
            ),
            patch("Empty", "01.02", 0, ""),
            patch("Chosen off", "01.02", 1, ""),
        ];
        let selection = Selection {
            enabled: vec![],
            disabled: vec!["community.xml/Chosen off".to_owned()],
        };
        let texts = Texts::new(Language::English);
        let rows = patch_rows(&found, &target(), &selection, &texts);
        let status: Vec<_> = rows.iter().map(|row| row.status.as_str()).collect();
        assert_eq!(
            status,
            [
                "On",
                "Not available: This patch is for a different title, executable or revision.",
                "Not available: pattern patches are not supported",
                "Not available: This patch has no supported literal writes.",
                "Off",
            ]
        );
        assert_eq!(rows[0].author, "by A & B");
        assert!(rows[0].available && !rows[1].available);

        // The reasons about_list gives itself are translated; the file's own are kept.
        let rows = patch_rows(&found, &target(), &selection, &Texts::new(Language::German));
        assert_eq!(rows[0].status, "An");
        assert!(!rows[1].status.contains(OTHER_BUILD), "{}", rows[1].status);
        assert!(rows[2]
            .status
            .ends_with("pattern patches are not supported"));
        assert!(!rows[3].status.contains(NO_WRITES), "{}", rows[3].status);
    }

    #[test]
    fn no_patches_means_no_rows() {
        let texts = Texts::new(Language::English);
        assert!(patch_rows(&[], &target(), &Selection::default(), &texts).is_empty());
    }
}
