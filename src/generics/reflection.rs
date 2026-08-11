use super::lsp::effective_inheritance_arguments;
use super::{
    GenericInheritanceKind, GenericMetadata, GenericReflectionBinding, GenericType, ReifiedBinding,
};

impl GenericMetadata {
    /// Materialize the effective runtime arguments retained by one reified
    /// object. Defaults are substituted through the arguments that precede
    /// them, so Reflection observes the concrete binding rather than the
    /// declaration's parameter references.
    pub fn reflection_reified_binding(
        &self,
        binding: ReifiedBinding,
    ) -> Option<GenericReflectionBinding> {
        let declaration = self.declaration(binding)?;
        let use_site = self.use_site(binding.use_site)?;
        Some(GenericReflectionBinding {
            arguments: effective_inheritance_arguments(declaration, &use_site.arguments)
                .into_boxed_slice(),
        })
    }

    /// Return one direct inheritance binding exactly as written by the owner,
    /// with omitted defaults materialized. This is a cold Reflection view over
    /// the same interned graph used by the linker.
    pub fn reflection_direct_binding(
        &self,
        owner: &str,
        kind: GenericInheritanceKind,
        ancestor: Option<&str>,
    ) -> Option<GenericReflectionBinding> {
        let inheritance = self.inheritances.iter().find(|inheritance| {
            inheritance.kind == kind
                && self
                    .symbol(inheritance.owner)
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(owner))
                && ancestor.is_none_or(|expected| {
                    self.symbol(inheritance.ancestor)
                        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(expected))
                })
        })?;
        let ancestor_name = self.symbol(inheritance.ancestor)?;
        let arguments = self.find_class_like(ancestor_name).map_or_else(
            || inheritance.arguments.to_vec(),
            |declaration| effective_inheritance_arguments(declaration, &inheritance.arguments),
        );
        Some(GenericReflectionBinding {
            arguments: arguments.into_boxed_slice(),
        })
    }

    /// Return every effective binding for one ancestor interface. Distinct
    /// paths with the same binding are collapsed by the linker traversal;
    /// distinct diamond bindings remain in source traversal order as required
    /// by RFC v0.22's plural Reflection result.
    pub fn reflection_interface_bindings(
        &self,
        owner: &str,
        ancestor: &str,
    ) -> Vec<GenericReflectionBinding> {
        let Some(child) = self.find_class_like(owner) else {
            return Vec::new();
        };
        let identity = (0..child.parameters.len())
            .map(|index| GenericType::Parameter(index as u8))
            .collect::<Vec<_>>();
        self.ancestor_bindings_from(child, &identity)
            .into_iter()
            .filter_map(|(declaration, binding)| {
                let candidate = self.symbol(declaration.owner)?;
                (candidate.eq_ignore_ascii_case(ancestor)).then(|| GenericReflectionBinding {
                    arguments: binding.into_boxed_slice(),
                })
            })
            .collect()
    }
}
