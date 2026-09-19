use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use syn::punctuated::Punctuated;
use syn::visit_mut::{self, VisitMut};
use syn::{Attribute, Item, Meta, Token};

pub fn load_modules(
    root: &Path,
    mut read: impl FnMut(&Path) -> Result<String, String>,
) -> Result<BTreeMap<String, syn::File>, String> {
    let text = read(root)?;
    let mut loader = ModuleLoader {
        read,
        modules: BTreeMap::new(),
        active: BTreeSet::new(),
    };
    loader.file("controller", root, &text)?;
    Ok(loader.modules)
}

pub fn controller_modules() -> BTreeMap<String, syn::File> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/adapter/controller/mod.rs");
    load_modules(&root, |path| {
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
    })
    .expect("controller module tree must parse")
}

struct ModuleLoader<R> {
    read: R,
    modules: BTreeMap<String, syn::File>,
    active: BTreeSet<PathBuf>,
}

impl<R: FnMut(&Path) -> Result<String, String>> ModuleLoader<R> {
    fn file(&mut self, name: &str, path: &Path, text: &str) -> Result<(), String> {
        if !self.active.insert(path.to_path_buf()) {
            return Err(format!("recursive module source: {}", path.display()));
        }
        let file = product_file(text).map_err(|error| format!("{}: {error}", path.display()))?;
        let parent = path
            .parent()
            .ok_or("module source needs a parent directory")?;
        let directory = if path.file_name().is_some_and(|file| file == "mod.rs") {
            parent.to_path_buf()
        } else {
            parent.join(path.file_stem().ok_or("module source needs a file name")?)
        };
        self.items(name, &file.items, &directory, parent)?;
        self.modules.insert(name.into(), file);
        self.active.remove(path);
        Ok(())
    }

    fn items(
        &mut self,
        name: &str,
        items: &[Item],
        directory: &Path,
        attribute_base: &Path,
    ) -> Result<(), String> {
        for item in items {
            let Item::Mod(module) = item else { continue };
            let child_name = format!("{name}::{}", module.ident);
            if self.modules.contains_key(&child_name) {
                return Err(format!("duplicate module candidate: {child_name}"));
            }
            let explicit_path = module
                .attrs
                .iter()
                .find(|attr| attr.path().is_ident("path"));
            if let Some((_, nested)) = &module.content {
                if explicit_path.is_some() {
                    return Err(format!(
                        "inline #[path] needs explicit support: {child_name}"
                    ));
                }
                let child_directory = directory.join(module.ident.to_string());
                self.items(&child_name, nested, &child_directory, &child_directory)?;
                self.modules.insert(
                    child_name,
                    syn::File {
                        shebang: None,
                        attrs: module.attrs.clone(),
                        items: nested.clone(),
                    },
                );
                continue;
            }
            let (path, text) = if let Some(attribute) = explicit_path {
                let Meta::NameValue(value) = &attribute.meta else {
                    return Err("#[path] needs a string".into());
                };
                let syn::Expr::Lit(value) = &value.value else {
                    return Err("#[path] needs a string literal".into());
                };
                let syn::Lit::Str(value) = &value.lit else {
                    return Err("#[path] needs a string literal".into());
                };
                let path = attribute_base.join(value.value());
                if path
                    .components()
                    .any(|component| component == std::path::Component::ParentDir)
                {
                    return Err(format!(
                        "parent path needs explicit support: {}",
                        path.display()
                    ));
                }
                let text = (self.read)(&path)?;
                (path, text)
            } else {
                let path = directory.join(format!("{}.rs", module.ident));
                match (self.read)(&path) {
                    Ok(text) => (path, text),
                    Err(first) => {
                        let path = directory.join(module.ident.to_string()).join("mod.rs");
                        let text =
                            (self.read)(&path).map_err(|second| format!("{first}; {second}"))?;
                        (path, text)
                    }
                }
            };
            self.file(&child_name, &path, &text)?;
        }
        Ok(())
    }
}

pub fn product_file(text: &str) -> syn::Result<syn::File> {
    let mut file = syn::parse_file(text)?;
    ProductItems.visit_file_mut(&mut file);
    Ok(file)
}

// Only `test` is fixed here. Unknown platform/feature predicates remain possible
// product code, so a boundary cannot disappear merely because the host differs.
fn test_configuration(meta: &Meta) -> Option<bool> {
    match meta {
        Meta::Path(path) if path.is_ident("test") => Some(false),
        Meta::List(list) => {
            let nested = list
                .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .expect("cfg predicate must be valid Rust metadata");
            let values: Vec<_> = nested.iter().map(test_configuration).collect();
            if list.path.is_ident("not") && values.len() == 1 {
                values[0].map(|value| !value)
            } else if list.path.is_ident("all") {
                if values.contains(&Some(false)) {
                    Some(false)
                } else if values.iter().all(|value| *value == Some(true)) {
                    Some(true)
                } else {
                    None
                }
            } else if list.path.is_ident("any") {
                if values.contains(&Some(true)) {
                    Some(true)
                } else if values.iter().all(|value| *value == Some(false)) {
                    Some(false)
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn product_attributes(attributes: &[Attribute]) -> bool {
    attributes.iter().all(|attribute| {
        !attribute.path().is_ident("cfg")
            || test_configuration(
                &attribute
                    .parse_args::<Meta>()
                    .expect("cfg attribute must contain a predicate"),
            ) != Some(false)
    })
}

fn product_item(item: &Item) -> bool {
    let attributes = match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => return true,
    };
    product_attributes(attributes)
}

struct ProductItems;

impl VisitMut for ProductItems {
    fn visit_file_mut(&mut self, file: &mut syn::File) {
        if !product_attributes(&file.attrs) {
            file.items.clear();
        }
        file.items.retain(product_item);
        visit_mut::visit_file_mut(self, file);
    }

    fn visit_item_mod_mut(&mut self, module: &mut syn::ItemMod) {
        if let Some((_, items)) = &mut module.content {
            items.retain(product_item);
        }
        visit_mut::visit_item_mod_mut(self, module);
    }

    fn visit_item_impl_mut(&mut self, implementation: &mut syn::ItemImpl) {
        implementation.items.retain(|item| match item {
            syn::ImplItem::Const(item) => product_attributes(&item.attrs),
            syn::ImplItem::Fn(item) => product_attributes(&item.attrs),
            syn::ImplItem::Type(item) => product_attributes(&item.attrs),
            syn::ImplItem::Macro(item) => product_attributes(&item.attrs),
            _ => true,
        });
        visit_mut::visit_item_impl_mut(self, implementation);
    }

    fn visit_item_trait_mut(&mut self, definition: &mut syn::ItemTrait) {
        definition.items.retain(|item| match item {
            syn::TraitItem::Const(item) => product_attributes(&item.attrs),
            syn::TraitItem::Fn(item) => product_attributes(&item.attrs),
            syn::TraitItem::Type(item) => product_attributes(&item.attrs),
            syn::TraitItem::Macro(item) => product_attributes(&item.attrs),
            _ => true,
        });
        visit_mut::visit_item_trait_mut(self, definition);
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        block.stmts.retain(|statement| match statement {
            syn::Stmt::Item(item) => product_item(item),
            syn::Stmt::Macro(item) => product_attributes(&item.attrs),
            _ => true,
        });
        visit_mut::visit_block_mut(self, block);
    }
}
