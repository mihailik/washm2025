use std::collections::{BTreeMap, BTreeSet};
use walrus::ir::*;
use walrus::*;

pub struct Instrumenter {
    pub coverage_ptr_id: Option<GlobalId>,
    pub probe_to_offset: BTreeMap<usize, u32>,
    pub module_id: Option<String>,
}

impl Instrumenter {
    pub fn new() -> Self {
        Self {
            coverage_ptr_id: None,
            probe_to_offset: BTreeMap::new(),
            module_id: None,
        }
    }

    pub fn find_and_patch_ptr(
        &mut self,
        module: &mut Module,
        dmz_start: u32,
    ) -> anyhow::Result<()> {
        // Find the __WASHM_COV_PTR global (exported by guest)
        let ptr_export = module.exports.iter().find(|e| e.name == "__WASHM_COV_PTR");

        if let Some(export) = ptr_export {
            if let ExportItem::Global(id) = export.item {
                self.coverage_ptr_id = Some(id);

                // Patch the initializer
                let global = module.globals.get_mut(id);
                if let GlobalKind::Local(init) = &mut global.kind {
                    *init = InitExpr::Value(Value::I32(dmz_start as i32));
                    println!(
                        "Patched __WASHM_COV_PTR to point to DMZ at 0x{:X}",
                        dmz_start
                    );
                }
                return Ok(());
            }
        }

        // If not found, create it (legacy fallback, but vision requires it to exist)
        Err(anyhow::anyhow!(
            "__WASHM_COV_PTR not found. Did you forget to export it?"
        ))
    }

    pub fn instrument(
        &mut self,
        module: &mut Module,
        mapping_points: &[u32],
    ) -> anyhow::Result<usize> {
        let ptr_id = self
            .coverage_ptr_id
            .ok_or_else(|| anyhow::anyhow!("Coverage pointer not found or not patched"))?;
        let mut probe_count = 0;

        let main_mem = module
            .memories
            .iter()
            .next()
            .map(|m| m.id())
            .ok_or_else(|| anyhow::anyhow!("No memory found"))?;

        // Extract module name/ID from DWARF or name section if possible
        // For now, we'll use a placeholder or check DWARF later
        self.module_id = Some("washm_test_guest".to_string());

        // Convert mapping points to a set for fast lookup
        let points_set: BTreeSet<u32> = mapping_points.iter().cloned().collect();

        println!(
            "Instrumenting module based on {} DWARF points...",
            points_set.len()
        );

        for (_id, func) in module.funcs.iter_local_mut() {
            let mut blocks_to_instrument = vec![func.entry_block()];

            let mut i = 0;
            while i < blocks_to_instrument.len() {
                let block_id = blocks_to_instrument[i];
                let block = func.block(block_id);
                for instr in block.instrs.iter() {
                    match &instr.0 {
                        Instr::Block(b) => blocks_to_instrument.push(b.seq),
                        Instr::Loop(l) => blocks_to_instrument.push(l.seq),
                        Instr::IfElse(if_instr) => {
                            blocks_to_instrument.push(if_instr.consequent);
                            blocks_to_instrument.push(if_instr.alternative);
                        }
                        _ => {}
                    }
                }
                i += 1;
            }

            for block_id in blocks_to_instrument {
                let block = func.block(block_id);

                // Find a precise DWARF anchor for this block
                let mut precise_offset = None;
                for (_, loc_id) in block.instrs.iter() {
                    let debug_str = format!("{:?}", loc_id);
                    let offset: u32 = debug_str
                        .trim_start_matches("InstrLocId(")
                        .trim_end_matches(")")
                        .parse()
                        .unwrap_or(0);

                    if points_set.contains(&offset) {
                        precise_offset = Some(offset);
                        break;
                    }
                }

                // If no precise DWARF match, fallback to first instruction loc for block identification
                let final_offset = precise_offset
                    .or_else(|| {
                        block.instrs.first().map(|(_, loc)| {
                            let debug_str = format!("{:?}", loc);
                            debug_str
                                .trim_start_matches("InstrLocId(")
                                .trim_end_matches(")")
                                .parse()
                                .unwrap_or(0)
                        })
                    })
                    .unwrap_or(0);

                self.probe_to_offset.insert(probe_count, final_offset);

                // We must insert the probe at the BEGINNING of the block!
                // To do this, we can take the block's current instructions, clear them,
                // add the probe, and then put them back!
                let old_instrs = func.block_mut(block_id).instrs.clone();
                func.block_mut(block_id).instrs.clear();

                let builder = func.builder_mut();
                let mut instrs_builder = builder.instr_seq(block_id);
                self.inject_probe(&mut instrs_builder, ptr_id, main_mem, probe_count);

                // Now push back the original instructions
                func.block_mut(block_id).instrs.extend(old_instrs);

                probe_count += 1;
            }
        }

        Ok(probe_count)
    }

    pub fn get_probe_mapping(&self) -> &BTreeMap<usize, u32> {
        &self.probe_to_offset
    }

    fn inject_probe(
        &self,
        instrs: &mut InstrSeqBuilder,
        ptr_id: GlobalId,
        mem_id: MemoryId,
        probe_id: usize,
    ) {
        instrs.global_get(ptr_id);
        instrs.i32_const(probe_id as i32);
        instrs.binop(BinaryOp::I32Add);
        instrs.i32_const(1);
        instrs.store(
            mem_id,
            StoreKind::I32_8 { atomic: false },
            MemArg {
                align: 1,
                offset: 0,
            },
        );
    }
}
