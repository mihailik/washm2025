use walrus::*;

pub struct FrontierShifter {
    pub coverage_size: u32,
}

impl FrontierShifter {
    pub fn new(coverage_size: u32) -> Self {
        Self { coverage_size }
    }

    pub fn shift(&self, module: &mut Module) -> anyhow::Result<u32> {
        // Place the DMZ at 256MB offset to avoid any collisions with the rust heap (which grows up from ~1MB)
        let new_base: u32 = 0x1000_0000;

        let memory_id = module
            .memories
            .iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No memory found"))?
            .id();

        let mem = module.memories.get_mut(memory_id);
        let required_bytes = new_base + self.coverage_size;
        let required_pages = required_bytes.div_ceil(65536);
        if mem.initial < required_pages {
            println!(
                "Expanding memory from {} to {} pages to fit DMZ at 256MB",
                mem.initial, required_pages
            );
            mem.initial = required_pages;
            if let Some(max) = mem.maximum {
                if max < required_pages {
                    mem.maximum = Some(required_pages);
                }
            }
        }

        let dmz_data = vec![0u8; self.coverage_size as usize];

        module.data.add(
            DataKind::Active(ActiveData {
                memory: memory_id,
                location: ActiveDataLocation::Absolute(new_base),
            }),
            dmz_data,
        );

        println!(
            "DMZ mapped at 0x{:X} (Size: {} bytes)",
            new_base, self.coverage_size
        );

        Ok(new_base)
    }
}
