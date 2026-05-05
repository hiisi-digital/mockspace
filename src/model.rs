use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApiVisibility {
    Public,
    Internal,
    Unspecified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateInfo {
    pub short_name: String,
    pub items: Vec<Item>,
    pub deps: Vec<String>,
    /// Macro invocations found in this crate (e.g. `define_signal!(KeyPressed ...)`)
    /// Each entry: (macro_name, generated_item_name, source_crate_short)
    pub macro_generated: Vec<MacroGenerated>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroGenerated {
    /// The macro being invoked, e.g. "define_signal"
    pub macro_name: String,
    /// The item name it generates, e.g. "KeyPressed"
    pub generated_name: String,
    /// The short crate name where the macro is defined, e.g. "signal"
    pub source_crate: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Trait(TraitItem),
    Struct(StructItem),
    Enum(EnumItem),
    Fn(FnItem),
    Macro(MacroItem),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitItem {
    pub name: String,
    pub generics: String,
    pub bounds: String,
    pub methods: Vec<FnSig>,
    pub visibility: ApiVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructItem {
    pub name: String,
    pub generics: String,
    #[allow(dead_code)]
    pub fields: Vec<Field>,
    pub visibility: ApiVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumItem {
    pub name: String,
    #[allow(dead_code)]
    pub variants: Vec<String>,
    pub visibility: ApiVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnItem {
    pub sig: FnSig,
    pub visibility: ApiVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroItem {
    pub name: String,
    pub is_proc: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnSig {
    pub name: String,
    pub generics: String,
    pub params: String,
    pub ret: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: String,
}

impl Item {
    pub fn name(&self) -> &str {
        match self {
            Item::Trait(t) => &t.name,
            Item::Struct(s) => &s.name,
            Item::Enum(e) => &e.name,
            Item::Fn(f) => &f.sig.name,
            Item::Macro(m) => &m.name,
        }
    }

    pub fn visibility(&self) -> ApiVisibility {
        match self {
            Item::Trait(t) => t.visibility,
            Item::Struct(s) => s.visibility,
            Item::Enum(e) => e.visibility,
            Item::Fn(f) => f.visibility,
            Item::Macro(_) => ApiVisibility::Public, // macros are always public
        }
    }
}

/// Map from (crate_dir_name) -> CrateInfo
pub type CrateMap = BTreeMap<String, CrateInfo>;
