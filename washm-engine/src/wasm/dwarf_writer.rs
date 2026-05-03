use crate::ir::expression::ExpressionId;
use crate::ir::module::Module;
use gimli::write as gw;

// Emit a minimal DWARF `.debug_line` (and related sections) using `gimli::write`.
// This produces a single compilation unit with a line program that maps
// emitted function instruction addresses to source locations stored in
// `module.annotations`.

pub fn emit_dwarf_sections(
    module: &Module,
    body_starts: &[usize],
    expr_offsets_per_func: &[std::collections::HashMap<ExpressionId, u32>],
) -> std::collections::HashMap<String, Vec<u8>> {
    let mut sections: std::collections::HashMap<String, Vec<u8>> = Default::default();

    // Encoding parameters: DWARF32, version 2 (minimal), address size = 4
    let encoding = gimli::Encoding {
        format: gimli::Format::Dwarf32,
        version: 2,
        address_size: 4,
    };

    // Create a DwarfUnit container
    let mut dwarf_unit = gw::DwarfUnit::new(encoding);

    // Prepare a LineProgram: comp_dir=".", comp_file="module"
    let comp_dir = gw::LineString::String(b".".to_vec());
    let comp_file = gw::LineString::String(b"module".to_vec());
    let line_encoding = gimli::LineEncoding::default();
    let mut line_program = gw::LineProgram::new(encoding, line_encoding, comp_dir, comp_file, None);

    // Single directory id
    let dir_id = line_program.add_directory(gw::LineString::String(b".".to_vec()));

    // Map module file_index -> FileId in LineProgram
    let mut file_table: std::collections::BTreeMap<u32, gw::FileId> = Default::default();

    for (func_idx, expr_map) in expr_offsets_per_func.iter().enumerate() {
        if func_idx >= body_starts.len() {
            log::warn!(
                "dwarf_writer: func_idx {} >= body_starts.len() {}",
                func_idx,
                body_starts.len()
            );
            continue;
        }
        // Collect annotated expressions for this function, sort by address
        let body_start = body_starts[func_idx] as u64;
        let mut rows: Vec<(u64, u32, u32)> = Vec::new(); // (addr, file_index, line)
        for (expr_id, &rel_off) in expr_map {
            if let Some(loc) = module.annotations.get_location(*expr_id) {
                let addr = body_start + rel_off as u64;
                rows.push((addr, loc.file_index, loc.line));
                file_table.entry(loc.file_index).or_insert_with(|| {
                    let fname = module
                        .debug_filenames
                        .get(loc.file_index as usize)
                        .filter(|s| !s.is_empty())
                        .cloned()
                        .unwrap_or_else(|| format!("file{}", loc.file_index));

                    line_program.add_file(gw::LineString::String(fname.into_bytes()), dir_id, None)
                });
            }
        }
        if rows.is_empty() {
            log::debug!(
                "dwarf_writer: func {} has no annotated rows (exprs in map: {})",
                func_idx,
                expr_map.len()
            );
            continue;
        }
        rows.sort_by_key(|r| r.0);

        // Debug: print the rows we will add
        log::debug!(
            "dwarf_writer: func {} rows (abs, file, line): {:?}",
            func_idx,
            rows
        );

        // Begin a sequence at the absolute function body start so emitted row.address()
        // is absolute and will match offsets recorded by the reader.
        let base = body_start;
        line_program.begin_sequence(Some(gw::Address::Constant(base)));
        for (addr, file_index, line) in rows.into_iter() {
            // addr is absolute; we want relative offset inside function body (row.address_offset)
            let rel = addr - body_start;
            let fid = *file_table.get(&file_index).unwrap();
            let row = line_program.row();
            row.address_offset = rel;
            row.file = fid;
            row.line = u64::from(line);
            // generate row instruction(s)
            line_program.generate_row();
        }
        // end sequence with last offset + 1 (exclusive end)
        let last_offset = line_program.row().address_offset;
        line_program.end_sequence(last_offset + 1);
    }

    // Assign the line program to the dwarf unit
    dwarf_unit.unit.line_program = line_program;

    // Prepare sections and write DWARF data
    let mut sections_out = gw::Sections::new(gw::EndianVec::new(gimli::LittleEndian));
    // Write dwarf
    if let Err(e) = dwarf_unit.write(&mut sections_out) {
        // On failure, just return empty map (we don't want writer to fail compilation)
        log::warn!("DWARF writer failed: {:?}", e);
        return sections;
    }

    // Extract section bytes
    let line_bytes = sections_out.debug_line.0.take();
    if !line_bytes.is_empty() {
        sections.insert(".debug_line".to_string(), line_bytes);
    }
    let info_bytes = sections_out.debug_info.0.take();
    if !info_bytes.is_empty() {
        sections.insert(".debug_info".to_string(), info_bytes);
    }
    let abbrev_bytes = sections_out.debug_abbrev.0.take();
    if !abbrev_bytes.is_empty() {
        sections.insert(".debug_abbrev".to_string(), abbrev_bytes);
    }
    let str_bytes = sections_out.debug_str.0.take();
    if !str_bytes.is_empty() {
        sections.insert(".debug_str".to_string(), str_bytes);
    }

    sections
}
