use super::*;
use crate::compiler::compile::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;

fn definition(source: &str) -> ClassDef {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compiled = Compiler::new().compile(&statements).unwrap();
    compiled
        .class_defs
        .into_iter()
        .chain(compiled.runtime_class_defs.into_iter().map(|(_, c)| c))
        .next()
        .unwrap()
}

fn register(eg: &mut ExecutorGlobals, source: &str) {
    eg.register_class(definition(source)).unwrap();
}

#[test]
fn resolved_metadata_preserves_trait_parent_and_unicode_precedence() {
    let mut eg = ExecutorGlobals::new();
    for source in [
        "<?php trait Leaf { protected function leaf() {} public function shared() {} public function K() {} }",
        "<?php trait Middle { use Leaf { leaf as public aliasLeaf; leaf as protected protectedLeaf; } }",
        "<?php class ParentMethods { private function hidden() {} protected static function base() {} public function shared() {} }",
        "<?php class ChildMethods extends ParentMethods { use Middle; public function local() {} }",
        "<?php abstract class AbstractMethods extends ParentMethods { abstract public function unfinished(); }",
    ] {
        register(&mut eg, source);
    }
    for class in [
        "Leaf",
        "Middle",
        "ParentMethods",
        "ChildMethods",
        "AbstractMethods",
    ] {
        for method in [
            "leaf",
            "LEAF",
            "aliasLeaf",
            "shared",
            "hidden",
            "base",
            "local",
            "unfinished",
            "missing",
            "k",
            "K",
            "K",
            "é",
            "",
        ] {
            assert_eq!(
                eg.find_method_info(class, method),
                eg.find_method_info_uncached(class, method),
                "{class}::{method}"
            );
        }
    }
    assert_eq!(
        eg.find_method_info("ChildMethods", "ALIASLEAF"),
        Some((Visibility::Public, false, "ChildMethods".into()))
    );
    assert_eq!(
        eg.find_method_info("ChildMethods", "hidden"),
        Some((Visibility::Private, false, "ParentMethods".into()))
    );
    assert_eq!(
        eg.find_method_info("ChildMethods", "base"),
        Some((Visibility::Protected, true, "ParentMethods".into()))
    );
}

#[test]
fn resolved_metadata_size_depends_on_declarations_not_misses_or_aliases() {
    let mut eg = ExecutorGlobals::new();
    register(
        &mut eg,
        "<?php class IndexedOwner { function present() {} }",
    );
    let id = eg.class_id_of("IndexedOwner");
    let index = Rc::clone(&eg.method_index_cache.borrow()[&id]);
    assert_eq!(index.resolved.get().unwrap().len(), 1);
    assert!(
        eg.register_class_alias("IndexedOwner", "IndexedAlias")
            .is_ok()
    );
    register(
        &mut eg,
        "<?php class UnrelatedOwner { function other() {} }",
    );
    for length in 1..=128 {
        let missing = "X".repeat(length);
        for owner in ["IndexedOwner", "indexedowner", "IndexedAlias"] {
            assert!(eg.find_method_info(owner, &missing).is_none());
            assert_eq!(
                eg.find_method_info(owner, "PRESENT"),
                Some((Visibility::Public, false, "IndexedOwner".into()))
            );
        }
    }
    assert_eq!(index.resolved.get().unwrap().len(), 1);
    assert!(Rc::ptr_eq(&index, &eg.method_index_cache.borrow()[&id]));
}

#[test]
fn resolved_metadata_retries_incomplete_hierarchy_after_registration() {
    let mut eg = ExecutorGlobals::new();
    register(
        &mut eg,
        "<?php class EarlyChild extends LateParent { function own() {} }",
    );
    assert!(eg.find_method_info("EarlyChild", "later").is_none());
    let id = eg.class_id_of("EarlyChild");
    let index = Rc::clone(&eg.method_index_cache.borrow()[&id]);
    assert!(index.resolved.get().is_none());
    register(
        &mut eg,
        "<?php class LateParent { protected static function later() {} }",
    );
    assert_eq!(
        eg.find_method_info("EarlyChild", "LATER"),
        Some((Visibility::Protected, true, "LateParent".into()))
    );
    assert!(index.resolved.get().is_some());
}

#[test]
fn resolved_metadata_native_mutations_invalidate_descendants() {
    let mut eg = ExecutorGlobals::new();
    register(&mut eg, "<?php class NativeOwner {}");
    register(&mut eg, "<?php class NativeChild extends NativeOwner {}");
    assert!(eg.find_method_info("NativeChild", "added").is_none());
    eg.register_internal_method_contract(
        "NativeOwner",
        "added",
        true,
        0,
        &[],
        Vec::new(),
        ParamTypeHint::Mixed,
        &[],
        false,
    );
    assert_eq!(
        eg.find_method_info("NativeChild", "added"),
        Some((Visibility::Public, true, "NativeOwner".into()))
    );
    eg.set_internal_method_access("NativeOwner", "added", Visibility::Protected, false);
    assert_eq!(
        eg.find_method_info("NativeChild", "ADDED"),
        Some((Visibility::Protected, true, "NativeOwner".into()))
    );
    eg.register_internal_method_contract(
        "NativeOwner",
        "second",
        false,
        0,
        &[],
        Vec::new(),
        ParamTypeHint::Mixed,
        &[],
        false,
    );
    eg.reorder_internal_method_contracts("NativeOwner", &["second", "added"]);
    for method in ["added", "second", "missing"] {
        assert_eq!(
            eg.find_method_info("NativeChild", method),
            eg.find_method_info_uncached("NativeChild", method)
        );
    }
}

#[test]
fn resolved_metadata_keeps_unregistered_classes_on_canonical_path() {
    let mut eg = ExecutorGlobals::new();
    let class = definition("<?php class UnregisteredMethods { protected function mixed() {} }");
    assert_eq!(class.class_id, 0);
    eg.class_table.insert(class.name.clone(), Rc::new(class));
    assert_eq!(
        eg.find_method_info("UnregisteredMethods", "MIXED"),
        Some((Visibility::Protected, false, "UnregisteredMethods".into()))
    );
    assert!(eg.method_index_cache.borrow().is_empty());
}

#[test]
fn resolved_metadata_does_not_eagerly_resolve_a_cyclic_native_graph() {
    let mut eg = ExecutorGlobals::new();
    let mut left = definition("<?php class CycleLeft { function own() {} }");
    let mut right = definition("<?php class CycleRight {}");
    left.class_id = 800;
    left.parent = Some("CycleRight".into());
    right.class_id = 801;
    right.parent = Some("CycleLeft".into());
    eg.class_table.insert(left.name.clone(), Rc::new(left));
    eg.class_table.insert(right.name.clone(), Rc::new(right));
    assert_eq!(
        eg.find_method_info("CycleLeft", "own"),
        Some((Visibility::Public, false, "CycleLeft".into()))
    );
    assert!(
        eg.method_index_cache.borrow()[&800]
            .resolved
            .get()
            .is_none()
    );
}
