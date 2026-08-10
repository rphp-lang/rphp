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
        let binding = self.active_reified_binding_scopes.remove(position).binding;
        self.remove_reified_binding(binding);
    }

    pub(crate) fn discard_pending_reified_binding_scopes(&mut self, owner: usize) {
        while let Some(position) = self
            .pending_reified_binding_scopes
            .iter()
            .rposition(|scope| scope.owner == owner)
        {
            let binding = self.pending_reified_binding_scopes.remove(position).binding;
            self.remove_reified_binding(binding);
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
