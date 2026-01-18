use crate::ir::wasm_features::FeatureSet;
use clap::{Arg, ArgAction, Command};
use std::io::Read;

pub fn read_input(path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
    if path.to_str() == Some("-") {
        let mut buffer = Vec::new();
        std::io::stdin().read_to_end(&mut buffer)?;
        Ok(buffer)
    } else {
        std::fs::read(path)
            .map_err(|e| anyhow::anyhow!("Failed to read input file {:?}: {}", path, e))
    }
}

pub fn read_input_string(path: &std::path::Path) -> anyhow::Result<String> {
    if path.to_str() == Some("-") {
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        Ok(buffer)
    } else {
        std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read input file {:?}: {}", path, e))
    }
}

pub fn write_output(path: &std::path::Path, data: &[u8]) -> anyhow::Result<()> {
    if path.to_str() == Some("-") {
        use std::io::Write;
        std::io::stdout().write_all(data)?;
        Ok(())
    } else {
        std::fs::write(path, data)
            .map_err(|e| anyhow::anyhow!("Failed to write output file {:?}: {}", path, e))
    }
}

pub fn add_feature_flags(
    mut cmd: Command,
) -> (Command, Vec<(FeatureSet, &'static str, &'static str)>) {
    let mut feature_flag_ids = Vec::new();

    cmd = cmd.arg(
        Arg::new("all-features")
            .long("all")
            .action(ArgAction::SetTrue)
            .help("Enable all features"),
    );

    for feature in FeatureSet::iter_all() {
        let name = FeatureSet::to_string(feature);
        let enable_name: &'static str = Box::leak(format!("enable-{}", name).into_boxed_str());
        let disable_name: &'static str = Box::leak(format!("disable-{}", name).into_boxed_str());
        feature_flag_ids.push((feature, enable_name, disable_name));

        cmd = cmd.arg(
            Arg::new(enable_name)
                .long(enable_name)
                .action(ArgAction::SetTrue)
                .help(format!("Enable {} feature", name)),
        );
        cmd = cmd.arg(
            Arg::new(disable_name)
                .long(disable_name)
                .action(ArgAction::SetTrue)
                .help(format!("Disable {} feature", name)),
        );
    }
    (cmd, feature_flag_ids)
}

pub fn apply_feature_flags(
    features: &mut FeatureSet,
    matches: &clap::ArgMatches,
    feature_flag_ids: &[(FeatureSet, &'static str, &'static str)],
) {
    if matches.get_flag("all-features") {
        *features = FeatureSet::all();
    }

    for (feature, enable_id, disable_id) in feature_flag_ids {
        if matches.get_flag(enable_id) {
            features.insert(*feature);
        }
        if matches.get_flag(disable_id) {
            features.remove(*feature);
        }
    }
}
