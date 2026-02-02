use bitflags::bitflags;

bitflags! {
    /// WebAssembly feature flags, matching Binaryen's internal FeatureSet.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct FeatureSet: u32 {
        const Atomics = 1 << 0;
        const MutableGlobals = 1 << 1;
        const TruncSat = 1 << 2;
        const SIMD = 1 << 3;
        const BulkMemory = 1 << 4;
        const SignExt = 1 << 5;
        const ExceptionHandling = 1 << 6;
        const TailCall = 1 << 7;
        const ReferenceTypes = 1 << 8;
        const Multivalue = 1 << 9;
        const GC = 1 << 10;
        const Memory64 = 1 << 11;
        const RelaxedSIMD = 1 << 12;
        const ExtendedConst = 1 << 13;
        const Strings = 1 << 14;
        const MultiMemory = 1 << 15;
        const StackSwitching = 1 << 16;
        const SharedEverything = 1 << 17;
        const FP16 = 1 << 18;
    }
}

impl FeatureSet {
    pub const NONE: Self = Self::empty();
    pub const MVP: Self = Self::NONE;
    pub const DEFAULT: Self =
        Self::from_bits_truncate(Self::SignExt.bits() | Self::MutableGlobals.bits());
    pub const ALL: Self = Self::from_bits_truncate((1 << 19) - 1);

    pub fn has(&self, feature: FeatureSet) -> bool {
        self.contains(feature)
    }

    pub fn enable(&mut self, feature: FeatureSet) {
        self.insert(feature);
    }

    pub fn disable(&mut self, feature: FeatureSet) {
        self.remove(feature);
    }

    pub fn iter_all() -> impl Iterator<Item = FeatureSet> {
        [
            Self::Atomics,
            Self::MutableGlobals,
            Self::TruncSat,
            Self::SIMD,
            Self::BulkMemory,
            Self::SignExt,
            Self::ExceptionHandling,
            Self::TailCall,
            Self::ReferenceTypes,
            Self::Multivalue,
            Self::GC,
            Self::Memory64,
            Self::RelaxedSIMD,
            Self::ExtendedConst,
            Self::Strings,
            Self::MultiMemory,
            Self::StackSwitching,
            Self::SharedEverything,
            Self::FP16,
        ]
        .into_iter()
    }

    /// Converts a feature flag to its standard string representation (e.g., "threads").
    pub fn to_string(f: FeatureSet) -> &'static str {
        if f == FeatureSet::Atomics {
            return "threads";
        }
        if f == FeatureSet::MutableGlobals {
            return "mutable-globals";
        }
        if f == FeatureSet::TruncSat {
            return "nontrapping-float-to-int";
        }
        if f == FeatureSet::SIMD {
            return "simd";
        }
        if f == FeatureSet::BulkMemory {
            return "bulk-memory";
        }
        if f == FeatureSet::SignExt {
            return "sign-ext";
        }
        if f == FeatureSet::ExceptionHandling {
            return "exception-handling";
        }
        if f == FeatureSet::TailCall {
            return "tail-call";
        }
        if f == FeatureSet::ReferenceTypes {
            return "reference-types";
        }
        if f == FeatureSet::Multivalue {
            return "multivalue";
        }
        if f == FeatureSet::GC {
            return "gc";
        }
        if f == FeatureSet::Memory64 {
            return "memory64";
        }
        if f == FeatureSet::RelaxedSIMD {
            return "relaxed-simd";
        }
        if f == FeatureSet::ExtendedConst {
            return "extended-const";
        }
        if f == FeatureSet::Strings {
            return "strings";
        }
        if f == FeatureSet::MultiMemory {
            return "multimemory";
        }
        if f == FeatureSet::StackSwitching {
            return "stack-switching";
        }
        if f == FeatureSet::SharedEverything {
            return "shared-everything";
        }
        if f == FeatureSet::FP16 {
            return "fp16";
        }
        "unknown"
    }

    /// Parses a standard feature string into its corresponding flag.
    pub fn from_string(s: &str) -> Option<FeatureSet> {
        match s {
            "threads" => Some(FeatureSet::Atomics),
            "mutable-globals" => Some(FeatureSet::MutableGlobals),
            "nontrapping-float-to-int" => Some(FeatureSet::TruncSat),
            "simd" => Some(FeatureSet::SIMD),
            "bulk-memory" => Some(FeatureSet::BulkMemory),
            "sign-ext" => Some(FeatureSet::SignExt),
            "exception-handling" => Some(FeatureSet::ExceptionHandling),
            "tail-call" => Some(FeatureSet::TailCall),
            "reference-types" => Some(FeatureSet::ReferenceTypes),
            "multivalue" => Some(FeatureSet::Multivalue),
            "gc" => Some(FeatureSet::GC),
            "memory64" => Some(FeatureSet::Memory64),
            "relaxed-simd" => Some(FeatureSet::RelaxedSIMD),
            "extended-const" => Some(FeatureSet::ExtendedConst),
            "strings" => Some(FeatureSet::Strings),
            "multimemory" => Some(FeatureSet::MultiMemory),
            "stack-switching" => Some(FeatureSet::StackSwitching),
            "shared-everything" => Some(FeatureSet::SharedEverything),
            "fp16" => Some(FeatureSet::FP16),
            _ => None,
        }
    }
}

impl Default for FeatureSet {
    fn default() -> Self {
        Self::DEFAULT
    }
}
