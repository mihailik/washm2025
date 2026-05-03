use gimli::{Dwarf, EndianSlice, LittleEndian, SectionId};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct SourceMapper {
    /// ID of the module (e.g. crate name)
    pub module_id: String,
    /// Maps probe ID to (source_file, line_number)
    pub probe_map: BTreeMap<usize, (String, u64)>,
    /// Intermediate map from binary offset to (source_file, line_number)
    #[serde(skip)]
    pub line_map: BTreeMap<u32, (String, u64)>,
}

impl SourceMapper {
    pub fn new(
        _module: &walrus::Module,
        original_bytes: &[u8],
        code_offset: u32,
    ) -> anyhow::Result<Self> {
        let mut line_map = BTreeMap::new();

        let mut loader = |id: SectionId| -> Result<Cow<[u8]>, ()> {
            let name = id.name();
            let wasm_name = name;

            // Use wasmparser to find the section in original bytes
            for payload in wasmparser::Parser::new(0).parse_all(original_bytes) {
                if let Ok(wasmparser::Payload::CustomSection(s)) = payload {
                    if s.name() == wasm_name || s.name() == &wasm_name[1..] {
                        return Ok(Cow::Borrowed(s.data()));
                    }
                }
            }
            Ok(Cow::Borrowed(&[]))
        };

        let dwarf_res = Dwarf::load(&mut loader);

        if let Err(e) = &dwarf_res {
            println!("Dwarf::load failed: {:?}", e);
        }

        if let Ok(dwarf) = dwarf_res {
            let dwarf = dwarf.borrow(|data| EndianSlice::new(data, LittleEndian));

            let mut iter = dwarf.units();
            while let Some(header) = iter.next()? {
                let unit = dwarf.unit(header)?;
                if let Some(program) = unit.line_program.clone() {
                    let mut rows = program.clone().rows();
                    while let Some((header, row)) = rows.next_row()? {
                        if row.is_stmt() {
                            let file_index = row.file_index();
                            if let Some(file) = header.file(file_index) {
                                let filename = dwarf
                                    .attr_string(&unit, file.path_name())
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_else(|_| "unknown".to_string());

                                let mut full_path = filename;
                                if let Some(dir) = file.directory(header) {
                                    if let Ok(dir_str) = dwarf.attr_string(&unit, dir) {
                                        let dir_str = dir_str.to_string_lossy().into_owned();
                                        full_path = format!("{}/{}", dir_str, full_path);
                                    }
                                }

                                let line = row.line().map(|l| l.get()).unwrap_or(0);
                                line_map
                                    .insert(row.address() as u32 + code_offset, (full_path, line));
                            }
                        }
                    }
                }
            }
        } else {
            println!("Warning: Could not load DWARF sections. Probes will be block-based only.");
        }

        println!(
            "Successfully parsed DWARF and mapped {} locations",
            line_map.len()
        );
        Ok(Self {
            module_id: "unknown".to_string(),
            probe_map: BTreeMap::new(),
            line_map,
        })
    }

    pub fn populate_probes(&mut self, probe_id: usize, offset: u32) {
        if let Some(info) = self.line_map.get(&offset) {
            self.probe_map.insert(probe_id, info.clone());
        }
    }

    pub fn all_mapping_points(&self) -> Vec<u32> {
        self.line_map.keys().cloned().collect()
    }
}
