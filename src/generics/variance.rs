use super::{
    GenericDeclaration, GenericMetadata, GenericType, GenericTypePosition, GenericVariance,
};

impl GenericMetadata {
    /// Validate declaration-site variance against the complete metadata graph
    /// of this compilation unit. Unknown nested generic declarations are left
    /// for a later link unit instead of being guessed invariantly.
    pub fn validate_variance(&self) -> Result<(), String> {
        for declaration in &self.declarations {
            self.validate_declaration_variance(declaration)?;
        }
        Ok(())
    }

    /// Re-run the class-like and method checks after separately compiled
    /// metadata has been merged, so a formerly unknown nested target can
    /// contribute its declared variance.
    pub fn validate_variance_for(&self, owner: &str) -> Result<(), String> {
        let method_prefix = format!("{}::", owner);
        for declaration in &self.declarations {
            let Some(declaration_owner) = self.symbol(declaration.owner) else {
                continue;
            };
            if declaration_owner.eq_ignore_ascii_case(owner)
                || (declaration.kind == super::GenericDeclarationKind::Method
                    && declaration_owner
                        .get(..method_prefix.len())
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&method_prefix)))
            {
                self.validate_declaration_variance(declaration)?;
            }
        }
        Ok(())
    }

    fn validate_declaration_variance(
        &self,
        declaration: &GenericDeclaration,
    ) -> Result<(), String> {
        for parameter in &declaration.parameters {
            if let Some(bound) = &parameter.bound {
                self.validate_type_position(declaration, bound, GenericTypePosition::Invariant)?;
            }
            if let Some(default) = &parameter.default {
                self.validate_type_position(declaration, default, GenericTypePosition::Invariant)?;
            }
        }
        for variance_use in &declaration.variance_uses {
            if variance_use.in_static_context && type_contains_parameter(&variance_use.value_type) {
                return Err(format!(
                    "Class-scope generic parameter cannot be used in static context of {} {}",
                    declaration.kind.label(),
                    self.symbol(declaration.owner).unwrap_or("?")
                ));
            }
            self.validate_type_position(
                declaration,
                &variance_use.value_type,
                variance_use.position,
            )?;
        }
        for inheritance in self
            .inheritances
            .iter()
            .filter(|inheritance| inheritance.owner == declaration.owner)
        {
            let Some(ancestor_name) = self.symbol(inheritance.ancestor) else {
                continue;
            };
            let Some(ancestor) = self.find_class_like(ancestor_name) else {
                continue;
            };
            for (index, argument) in inheritance.arguments.iter().enumerate() {
                let position = ancestor
                    .parameters
                    .get(index)
                    .map_or(GenericTypePosition::Invariant, |parameter| {
                        GenericTypePosition::from_variance(parameter.variance)
                    });
                self.validate_type_position(declaration, argument, position)?;
            }
        }
        Ok(())
    }

    fn validate_type_position(
        &self,
        declaration: &GenericDeclaration,
        value: &GenericType,
        position: GenericTypePosition,
    ) -> Result<(), String> {
        match value {
            GenericType::Parameter(index) => {
                let Some(parameter) = declaration.parameters.get(*index as usize) else {
                    return Ok(());
                };
                let valid = match parameter.variance {
                    GenericVariance::Invariant => true,
                    GenericVariance::Covariant => position == GenericTypePosition::Covariant,
                    GenericVariance::Contravariant => {
                        position == GenericTypePosition::Contravariant
                    }
                };
                if valid {
                    return Ok(());
                }
                let variance = match parameter.variance {
                    GenericVariance::Invariant => "Invariant",
                    GenericVariance::Covariant => "Covariant",
                    GenericVariance::Contravariant => "Contravariant",
                };
                return Err(format!(
                    "{} generic parameter {} of {} {} cannot be used in {} position",
                    variance,
                    self.symbol(parameter.name).unwrap_or("?"),
                    declaration.kind.label(),
                    self.symbol(declaration.owner).unwrap_or("?"),
                    position.label()
                ));
            }
            GenericType::Named { name, arguments } => {
                if arguments.is_empty() {
                    return Ok(());
                }
                let Some(target_name) = self.symbol(*name) else {
                    return Ok(());
                };
                let Some(target) = self.find_class_like(target_name) else {
                    // Cross-unit target: its declared parameter variance is not
                    // yet available, so preserve the metadata and defer.
                    return Ok(());
                };
                for (index, argument) in arguments.iter().enumerate() {
                    let nested = target
                        .parameters
                        .get(index)
                        .map_or(GenericTypePosition::Invariant, |parameter| {
                            position.compose(parameter.variance)
                        });
                    self.validate_type_position(declaration, argument, nested)?;
                }
            }
            GenericType::Nullable(inner) => {
                self.validate_type_position(declaration, inner, position)?;
            }
            GenericType::Union(parts) => {
                for part in parts {
                    self.validate_type_position(declaration, part, position)?;
                }
            }
            GenericType::Int
            | GenericType::Float
            | GenericType::String
            | GenericType::Bool
            | GenericType::Array
            | GenericType::Callable
            | GenericType::Null
            | GenericType::Void
            | GenericType::Mixed
            | GenericType::Never => {}
        }
        Ok(())
    }
}

impl GenericTypePosition {
    fn from_variance(variance: GenericVariance) -> Self {
        match variance {
            GenericVariance::Invariant => Self::Invariant,
            GenericVariance::Covariant => Self::Covariant,
            GenericVariance::Contravariant => Self::Contravariant,
        }
    }

    fn compose(self, variance: GenericVariance) -> Self {
        match (self, variance) {
            (Self::Invariant, _) | (_, GenericVariance::Invariant) => Self::Invariant,
            (Self::Covariant, GenericVariance::Covariant)
            | (Self::Contravariant, GenericVariance::Contravariant) => Self::Covariant,
            (Self::Covariant, GenericVariance::Contravariant)
            | (Self::Contravariant, GenericVariance::Covariant) => Self::Contravariant,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Covariant => "covariant",
            Self::Contravariant => "contravariant",
            Self::Invariant => "invariant",
        }
    }
}

fn type_contains_parameter(value: &GenericType) -> bool {
    match value {
        GenericType::Parameter(_) => true,
        GenericType::Named { arguments, .. } => arguments.iter().any(type_contains_parameter),
        GenericType::Nullable(inner) => type_contains_parameter(inner),
        GenericType::Union(parts) => parts.iter().any(type_contains_parameter),
        GenericType::Int
        | GenericType::Float
        | GenericType::String
        | GenericType::Bool
        | GenericType::Array
        | GenericType::Callable
        | GenericType::Null
        | GenericType::Void
        | GenericType::Mixed
        | GenericType::Never => false,
    }
}
