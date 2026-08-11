use super::ExecutorGlobals;
use crate::generics::ReifiedBinding;

#[derive(Clone, Copy)]
pub(super) struct PendingReifiedBindingScope {
    pub(super) owner: usize,
    pub(super) binding: ReifiedBinding,
}

#[derive(Clone, Copy)]
pub(super) struct ActiveReifiedBindingScope {
    pub(super) owner: usize,
    pub(super) call: usize,
    pub(super) binding: ReifiedBinding,
}

impl ExecutorGlobals {
    pub(crate) fn push_reified_binding_scope(&mut self, owner: usize, binding: ReifiedBinding) {
        self.reified_bindings.push(binding);
        self.pending_reified_binding_scopes
            .push(PendingReifiedBindingScope { owner, binding });
    }

    pub(crate) fn push_reified_binding_scope_with_class(
        &mut self,
        owner: usize,
        binding: ReifiedBinding,
        class_id: u32,
    ) {
        self.push_reified_binding_scope(owner, binding);
        if class_id != 0 {
            self.reified_binding_scope_classes.push((owner, class_id));
        }
    }

    pub(crate) fn activate_reified_binding_scope(&mut self, owner: usize, call: usize) {
        let Some(position) = self
            .pending_reified_binding_scopes
            .iter()
            .rposition(|scope| scope.owner == owner)
        else {
            return;
        };
        let pending = self.pending_reified_binding_scopes.remove(position);
        self.active_reified_binding_scopes
            .push(ActiveReifiedBindingScope {
                owner,
                call,
                binding: pending.binding,
            });
    }

    pub(crate) fn reified_binding_scope_class_id(&self, owner: usize) -> u32 {
        self.reified_binding_scope_classes
            .iter()
            .rfind(|(scope_owner, _)| *scope_owner == owner)
            .map(|(_, class_id)| *class_id)
            .unwrap_or(0)
    }

    pub(crate) fn finish_reified_binding_scope(&mut self, owner: usize) {
        let binding = self
            .active_reified_binding_scopes
            .iter()
            .rposition(|scope| scope.owner == owner)
            .map(|position| self.active_reified_binding_scopes.remove(position).binding)
            .or_else(|| {
                self.pending_reified_binding_scopes
                    .iter()
                    .rposition(|scope| scope.owner == owner)
                    .map(|position| self.pending_reified_binding_scopes.remove(position).binding)
            });
        if let Some(binding) = binding {
            self.remove_reified_binding_scope_class(owner);
            self.remove_reified_binding(binding);
        }
    }

    pub(crate) fn discard_active_reified_binding_scope(&mut self, call: usize) {
        let Some(position) = self
            .active_reified_binding_scopes
            .iter()
            .rposition(|scope| scope.call == call)
        else {
            return;
        };
        let scope = self.active_reified_binding_scopes.remove(position);
        let binding = scope.binding;
        self.remove_reified_binding_scope_class(scope.owner);
        self.remove_reified_binding(binding);
    }

    pub(crate) fn discard_pending_reified_binding_scopes(&mut self, owner: usize) {
        while let Some(position) = self
            .pending_reified_binding_scopes
            .iter()
            .rposition(|scope| scope.owner == owner)
        {
            let binding = self.pending_reified_binding_scopes.remove(position).binding;
            self.remove_reified_binding_scope_class(owner);
            self.remove_reified_binding(binding);
        }
    }

    fn remove_reified_binding_scope_class(&mut self, owner: usize) {
        if let Some(position) = self
            .reified_binding_scope_classes
            .iter()
            .rposition(|(scope_owner, _)| *scope_owner == owner)
        {
            self.reified_binding_scope_classes.remove(position);
        }
    }

    fn remove_reified_binding(&mut self, binding: ReifiedBinding) {
        if self.reified_bindings.last() == Some(&binding) {
            self.reified_bindings.pop();
            return;
        }
        if let Some(position) = self
            .reified_bindings
            .iter()
            .rposition(|candidate| *candidate == binding)
        {
            self.reified_bindings.remove(position);
        }
    }
}
