use crate::parser::{
    Attribute, BinOp, CallArg, CastType, ClassConstant, ClassMethod, ClassProperty, EnumCase, Expr,
    ForeachTarget, GenericAncestor, GlobalTarget, ListTarget, Param, Stmt, TypeHint, Visibility,
};

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
fn simple_interpolation_parts_supported(expr: &Expr) -> bool {
    match expr {
        Expr::StringLiteral(_) | Expr::Variable { .. } => true,
        Expr::BinaryOp {
            op: BinOp::Concat,
            left,
            right,
            ..
        } => {
            simple_interpolation_parts_supported(left)
                && simple_interpolation_parts_supported(right)
        }
        _ => false,
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
fn interpolation_following_requires_braces(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'[') || byte >= 0x80
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
fn simple_interpolation_leading_byte(expr: &Expr) -> Option<u8> {
    match expr {
        Expr::StringLiteral(value) => value.as_bytes().first().copied(),
        Expr::Variable { .. } => Some(b'$'),
        Expr::BinaryOp {
            op: BinOp::Concat,
            left,
            right,
            ..
        } => simple_interpolation_leading_byte(left)
            .or_else(|| simple_interpolation_leading_byte(right)),
        _ => None,
    }
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
fn write_simple_interpolation_parts_with_following(
    output: &mut String,
    expr: &Expr,
    following: Option<u8>,
) -> Option<()> {
    match expr {
        Expr::StringLiteral(value) => output.push_str(value),
        Expr::Variable { name, .. } => {
            if following.is_some_and(interpolation_following_requires_braces) {
                output.push_str("{$");
                output.push_str(name);
                output.push('}');
            } else {
                output.push('$');
                output.push_str(name);
            }
        }
        Expr::BinaryOp {
            op: BinOp::Concat,
            left,
            right,
            ..
        } => {
            let right_leading = simple_interpolation_leading_byte(right).or(following);
            write_simple_interpolation_parts_with_following(output, left, right_leading)?;
            write_simple_interpolation_parts_with_following(output, right, following)?;
        }
        _ => return None,
    }
    Some(())
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
fn write_simple_interpolation_parts(output: &mut String, expr: &Expr) -> Option<()> {
    write_simple_interpolation_parts_with_following(output, expr, None)
}

#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
pub(super) fn simple_assertion_expression_source(expr: &Expr) -> Option<String> {
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn binary_operator(op: &BinOp) -> Option<(&'static str, u8, bool)> {
        Some(match op {
            BinOp::Or => ("||", 10, false),
            BinOp::And => ("&&", 20, false),
            BinOp::BitwiseOr => ("|", 25, false),
            BinOp::BitwiseXor => ("^", 26, false),
            BinOp::BitwiseAnd => ("&", 27, false),
            BinOp::Equal => ("==", 30, false),
            BinOp::NotEqual => ("!=", 30, false),
            BinOp::Identical => ("===", 30, false),
            BinOp::NotIdentical => ("!==", 30, false),
            BinOp::Less => ("<", 30, false),
            BinOp::LessEqual => ("<=", 30, false),
            BinOp::Greater => (">", 30, false),
            BinOp::GreaterEqual => (">=", 30, false),
            BinOp::Concat => (".", 50, false),
            BinOp::ShiftLeft => ("<<", 55, false),
            BinOp::ShiftRight => (">>", 55, false),
            BinOp::Add => ("+", 60, false),
            BinOp::Sub => ("-", 60, false),
            BinOp::Mul => ("*", 70, false),
            BinOp::Div => ("/", 70, false),
            BinOp::Mod => ("%", 70, false),
            BinOp::Pow => ("**", 80, true),
            _ => return None,
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn precedence(expr: &Expr) -> Option<u8> {
        Some(match expr {
            Expr::Integer(_) | Expr::Bool(_) | Expr::Null | Expr::Variable { .. } => 100,
            Expr::StringLiteral(value) if simple_single_quoted_string(value) => 100,
            Expr::InterpolatedString {
                source: None,
                value,
                ..
            } if simple_interpolation_parts_supported(value) => 100,
            Expr::InterpolatedString {
                source: Some(source),
                ..
            } if !source.contains("${") && !source.contains("{$") && !source.contains('[') => 100,
            Expr::Constant { name, .. }
                if !name.eq_ignore_ascii_case("exit") && !name.eq_ignore_ascii_case("die") =>
            {
                100
            }
            Expr::Not(_) | Expr::UnaryPlus(_) | Expr::UnaryMinus(_) | Expr::BitwiseNot { .. } => 80,
            Expr::BinaryOp { op, .. } => binary_operator(op)?.1,
            _ => return None,
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn simple_single_quoted_string(value: &str) -> bool {
        for byte in value.as_bytes() {
            if matches!(byte, b'\\' | b'\'') {
                return false;
            }
        }
        true
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render(
        output: &mut String,
        expr: &Expr,
        parent_precedence: u8,
        right_child: bool,
    ) -> Option<()> {
        use std::fmt::Write as _;

        let expression_precedence = precedence(expr)?;
        let parenthesized = expression_precedence < parent_precedence
            || (right_child && expression_precedence == parent_precedence);
        if parenthesized {
            output.push('(');
        }
        match expr {
            Expr::Integer(value) => write!(output, "{value}").ok()?,
            Expr::StringLiteral(value) => {
                output.push('\'');
                output.push_str(value);
                output.push('\'');
            }
            Expr::InterpolatedString {
                source: Some(source),
                ..
            } => {
                output.push('"');
                output.push_str(source);
                output.push('"');
            }
            Expr::InterpolatedString {
                source: None,
                value,
                ..
            } => {
                output.push('"');
                write_simple_interpolation_parts(output, value)?;
                output.push('"');
            }
            Expr::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            Expr::Null => output.push_str("null"),
            Expr::Variable { name, .. } => {
                output.push('$');
                output.push_str(name);
            }
            Expr::Constant { name, .. } => output.push_str(name),
            Expr::Not(value) => {
                output.push('!');
                render(output, value, 80, false)?;
            }
            Expr::UnaryPlus(value) => {
                output.push('+');
                render(output, value, 80, false)?;
            }
            Expr::UnaryMinus(value) => {
                output.push('-');
                render(output, value, 80, false)?;
            }
            Expr::BitwiseNot { expr, .. } => {
                output.push('~');
                render(output, expr, 80, false)?;
            }
            Expr::BinaryOp {
                op, left, right, ..
            } => {
                let (operator, precedence, right_associative) = binary_operator(op)?;
                render(output, left, precedence, right_associative)?;
                output.push(' ');
                output.push_str(operator);
                output.push(' ');
                render(output, right, precedence, !right_associative)?;
            }
            _ => return None,
        }
        if parenthesized {
            output.push(')');
        }
        Some(())
    }

    let mut source = String::with_capacity(64);
    source.push_str("assert(");
    render(&mut source, expr, 0, false)?;
    source.push(')');
    Some(source)
}

#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
pub(crate) fn assertion_expression_source(expr: &Expr) -> Option<String> {
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn quote_string(value: &str) -> String {
        format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn quote_binary_string(value: &str) -> String {
        let mut quoted = String::from("\"");
        for character in value.chars() {
            let byte = character as u8;
            quoted.push_str(&format!("\\x{byte:02X}"));
        }
        quoted.push('"');
        quoted
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_interpolated_string(source: Option<&str>, value: &Expr) -> Option<String> {
        #[cold]
        #[inline(never)]
        #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
        fn plain_variable(source: &str) -> bool {
            let bytes = source.as_bytes();
            if bytes.first() != Some(&b'$')
                || !bytes
                    .get(1)
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
            {
                return false;
            }
            for byte in &bytes[2..] {
                if !byte.is_ascii_alphanumeric() && *byte != b'_' {
                    return false;
                }
            }
            true
        }

        let Some(source) = source else {
            let mut output = String::with_capacity(32);
            output.push('"');
            write_simple_interpolation_parts(&mut output, value)?;
            output.push('"');
            return Some(output);
        };
        // Most interpolated strings already use PHP's canonical spelling.
        // Only legacy/braced forms and bare array offsets need the slower
        // normalization pass below.
        if !source.contains("${") && !source.contains("{$") && !source.contains('[') {
            let mut output = String::with_capacity(source.len() + 2);
            output.push('"');
            output.push_str(source);
            output.push('"');
            return Some(output);
        }
        let bytes = source.as_bytes();
        let mut output = String::from("\"");
        let mut position = 0;
        while position < bytes.len() {
            if bytes[position] == b'\\' && position + 1 < bytes.len() {
                output.push('\\');
                position += 1;
                let escaped = source[position..]
                    .chars()
                    .next()
                    .expect("valid UTF-8 source");
                output.push(escaped);
                position += escaped.len_utf8();
                continue;
            }
            if bytes[position] == b'{' && bytes.get(position + 1) == Some(&b'$') {
                let mut end = position + 2;
                let mut depth = 1_u32;
                while end < bytes.len() && depth != 0 {
                    match bytes[end] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    end += 1;
                }
                if depth == 0 {
                    let inner = &source[position + 1..end - 1];
                    let following_requires_braces = bytes
                        .get(end)
                        .copied()
                        .is_some_and(interpolation_following_requires_braces);
                    if plain_variable(inner) && !following_requires_braces {
                        output.push_str(inner);
                    } else {
                        output.push_str(&source[position..end]);
                    }
                    position = end;
                    continue;
                }
            }
            if bytes.get(position..position + 2) == Some(b"${") {
                let mut end = position + 2;
                let mut depth = 1_u32;
                while end < bytes.len() && depth != 0 {
                    match bytes[end] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    end += 1;
                }
                if depth == 0 {
                    let inner = &source[position + 2..end - 1];
                    if plain_identifier(inner) {
                        output.push_str("{$");
                        output.push_str(inner);
                        output.push('}');
                    } else if legacy_variable_array_access(inner) {
                        output.push_str("{$");
                        output.push_str(inner);
                        output.push('}');
                    } else if inner.starts_with('$') {
                        output.push_str("{${");
                        output.push_str(inner);
                        output.push_str("}}");
                    } else if inner.len() >= 2 && inner.starts_with('\'') && inner.ends_with('\'') {
                        output.push_str("${");
                        output.push_str(&inner[1..inner.len() - 1]);
                        output.push('}');
                    } else {
                        output.push_str(&source[position..end]);
                    }
                    position = end;
                    continue;
                }
            }
            if bytes[position] == b'$'
                && bytes
                    .get(position + 1)
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
            {
                let mut end = position + 2;
                while bytes
                    .get(end)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    end += 1;
                }
                if bytes.get(end) == Some(&b'[') {
                    let mut bracket_end = end + 1;
                    while bytes.get(bracket_end).is_some_and(|byte| *byte != b']') {
                        bracket_end += 1;
                    }
                    if bytes.get(bracket_end) == Some(&b']') {
                        output.push('{');
                        output.push_str(&source[position..=bracket_end]);
                        output.push('}');
                        position = bracket_end + 1;
                        continue;
                    }
                }
            }
            let character = source[position..]
                .chars()
                .next()
                .expect("valid UTF-8 source");
            output.push(character);
            position += character.len_utf8();
        }
        output.push('"');
        Some(output)
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn plain_identifier(value: &str) -> bool {
        let bytes = value.as_bytes();
        if !bytes
            .first()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            return false;
        }
        for byte in bytes {
            if !byte.is_ascii_alphanumeric() && *byte != b'_' {
                return false;
            }
        }
        true
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn legacy_variable_array_access(value: &str) -> bool {
        let bytes = value.as_bytes();
        let name_end = bytes
            .iter()
            .position(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
            .unwrap_or(bytes.len());
        if !plain_identifier(&value[..name_end]) || bytes.get(name_end) != Some(&b'[') {
            return false;
        }

        let mut position = name_end;
        while position < bytes.len() {
            if bytes[position] != b'[' {
                return false;
            }
            let content_start = position + 1;
            let Some(relative_end) = bytes[content_start..].iter().position(|byte| *byte == b']')
            else {
                return false;
            };
            let content_end = content_start + relative_end;
            if content_end == content_start
                || bytes[content_start..content_end]
                    .iter()
                    .any(|byte| matches!(*byte, b'[' | b'{' | b'}' | b'\\'))
            {
                return false;
            }
            position = content_end + 1;
        }
        true
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_float(value: f64) -> String {
        let rendered = value.to_string();
        if value.is_finite()
            && !rendered.contains('.')
            && !rendered.contains('e')
            && !rendered.contains('E')
        {
            format!("{rendered}.0")
        } else {
            rendered
        }
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_type_hint(hint: &TypeHint) -> Option<String> {
        Some(match hint {
            TypeHint::Int => "int".to_string(),
            TypeHint::Float => "float".to_string(),
            TypeHint::String => "string".to_string(),
            TypeHint::Bool => "bool".to_string(),
            TypeHint::Array => "array".to_string(),
            TypeHint::Callable => "callable".to_string(),
            TypeHint::Null => "null".to_string(),
            TypeHint::Void => "void".to_string(),
            TypeHint::Mixed => "mixed".to_string(),
            TypeHint::Never => "never".to_string(),
            TypeHint::ClassName(name) => name.clone(),
            TypeHint::Nullable(inner) => format!("?{}", render_type_hint(inner)?),
            TypeHint::Union(members) => {
                let mut output = String::new();
                for (index, member) in members.iter().enumerate() {
                    if index != 0 {
                        output.push('|');
                    }
                    output.push_str(&render_type_hint(member)?);
                }
                output
            }
            TypeHint::Intersection(members) => {
                let mut output = String::new();
                for (index, member) in members.iter().enumerate() {
                    if index != 0 {
                        output.push('&');
                    }
                    output.push_str(&render_type_hint(member)?);
                }
                output
            }
            TypeHint::GenericParameter { .. } | TypeHint::GenericApplication { .. } => {
                return None;
            }
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_arguments(arguments: &[CallArg]) -> Option<String> {
        let mut output = String::new();
        for (index, argument) in arguments.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            match argument {
                CallArg::Positional(value) => output.push_str(&render(value, 0, false)?),
                CallArg::Named { name, value } => {
                    output.push_str(name);
                    output.push_str(": ");
                    output.push_str(&render(value, 0, false)?);
                }
                CallArg::Unpack(value) => {
                    output.push_str("...");
                    output.push_str(&render(value, 0, false)?);
                }
            }
        }
        Some(output)
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_ancestor_names(ancestors: &[GenericAncestor]) -> String {
        let mut output = String::new();
        for (index, ancestor) in ancestors.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            output.push_str(&ancestor.name);
        }
        output
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_array(elements: &[crate::parser::ArrayElement]) -> Option<String> {
        let mut output = String::from("[");
        for (index, element) in elements.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            let value = render(&element.value, 0, false)?;
            if element.unpack {
                output.push_str("...");
                output.push_str(&value);
                continue;
            }
            if let Some(key) = &element.key {
                output.push_str(&render(key, 0, false)?);
                output.push_str(" => ");
            }
            if element.by_reference {
                output.push('&');
            }
            output.push_str(&value);
        }
        output.push(']');
        Some(output)
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_dynamic_variable(name: &Expr) -> Option<String> {
        match name {
            Expr::Variable { name, .. } => Some(format!("$${name}")),
            _ => Some(format!("${{{}}}", render(name, 0, false)?)),
        }
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_parameter(parameter: &Param) -> Option<String> {
        let mut output = String::new();
        let attributes = render_attributes(&parameter.attributes)?;
        if !attributes.is_empty() {
            output.push_str(&attributes);
            output.push(' ');
        }
        if let Some((visibility, set_visibility, readonly)) = parameter.promotion {
            output.push_str(visibility_source(visibility));
            output.push(' ');
            if let Some(set_visibility) = set_visibility {
                output.push_str(visibility_source(set_visibility));
                output.push_str("(set) ");
            }
            if parameter
                .promoted_property
                .as_ref()
                .is_some_and(|property| property.is_final)
            {
                output.push_str("final ");
            }
            if readonly {
                output.push_str("readonly ");
            }
        }
        if let Some(hint) = &parameter.type_hint {
            output.push_str(&render_type_hint(hint)?);
            output.push(' ');
        }
        if parameter.is_ref {
            output.push('&');
        }
        if parameter.is_variadic {
            output.push_str("...");
        }
        output.push('$');
        output.push_str(&parameter.name);
        if let Some(default) = &parameter.default {
            output.push_str(" = ");
            output.push_str(&render(default, 0, false)?);
        }
        if let Some(property) = &parameter.promoted_property
            && !parameter.promotion_hooks.is_empty()
        {
            output.push(' ');
            output.push_str(&render_property_hook_list(
                property,
                &parameter.promotion_hooks,
            )?);
        }
        Some(output)
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_attribute_groups(attributes: &[Attribute]) -> Option<Vec<String>> {
        for attribute in attributes {
            if attribute.is_non_enum_case_marker() {
                return None;
            }
        }
        let mut groups: Vec<Vec<String>> = Vec::new();
        for attribute in attributes {
            if attribute.is_assertion_group_marker() {
                if let Some(group) = groups.last()
                    && !group.is_empty()
                {
                    groups.push(Vec::new());
                }
                continue;
            }
            let arguments = if attribute.args.is_empty() {
                String::new()
            } else {
                format!("({})", render_arguments(&attribute.args)?)
            };
            let rendered = format!("{}{arguments}", attribute.name);
            if let Some(group) = groups.last_mut() {
                group.push(rendered);
            } else {
                groups.push(vec![rendered]);
            }
        }
        let mut output = Vec::with_capacity(groups.len());
        for members in groups {
            output.push(format!("#[{}]", members.join(", ")));
        }
        Some(output)
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_attributes(attributes: &[Attribute]) -> Option<String> {
        let groups = render_attribute_groups(attributes)?;
        Some(groups.join(" "))
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_attribute_lines(attributes: &[Attribute]) -> Option<String> {
        let groups = render_attribute_groups(attributes)?;
        Some(groups.join("\n"))
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn visibility_source(visibility: Visibility) -> &'static str {
        match visibility {
            Visibility::Public => "public",
            Visibility::Protected => "protected",
            Visibility::Private => "private",
        }
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_property_hook(method: &ClassMethod) -> Option<String> {
        let kind = method.name.rsplit("::").next()?;
        let attributes = render_attribute_lines(&method.attributes)?;
        let mut prefix = String::new();
        if method.is_final {
            prefix.push_str("final ");
        }
        if method.returns_by_ref {
            prefix.push('&');
        }
        prefix.push_str(kind);
        let mut explicit_parameters = false;
        for parameter in &method.params {
            if parameter.name.starts_with('\0')
                || (kind.eq_ignore_ascii_case("set") && parameter.name != "value")
            {
                explicit_parameters = true;
                break;
            }
        }
        if explicit_parameters {
            let mut parameters = String::new();
            for parameter in &method.params {
                if parameter.name.starts_with('\0') {
                    continue;
                }
                if !parameters.is_empty() {
                    parameters.push_str(", ");
                }
                parameters.push_str(&render_parameter(parameter)?);
            }
            prefix.push('(');
            prefix.push_str(&parameters);
            prefix.push(')');
        }
        let declaration = if method.is_abstract || !method.has_body {
            format!("{prefix};")
        } else if method.body.len() == 1 {
            match &method.body[0] {
                Stmt::Return {
                    expr: Some(value),
                    line,
                } if kind.eq_ignore_ascii_case("get") && *line == method.line => {
                    format!("{prefix} => {};", render(value, 0, false)?)
                }
                Stmt::AssignProp { expr, line, .. }
                    if kind.eq_ignore_ascii_case("set") && *line == method.line =>
                {
                    format!("{prefix} => {};", render(expr, 0, false)?)
                }
                _ => format!("{prefix} {{\n{}}}", render_block(&method.body)?),
            }
        } else {
            format!("{prefix} {{\n{}}}", render_block(&method.body)?)
        };
        Some(if attributes.is_empty() {
            declaration
        } else {
            format!("{attributes}\n{declaration}")
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_property_hook_list(
        property: &ClassProperty,
        methods: &[ClassMethod],
    ) -> Option<String> {
        let prefix = format!("${}::", property.name);
        let mut hooks = Vec::new();
        for method in methods {
            if method.name.starts_with(&prefix) {
                hooks.push(render_property_hook(method)?);
            }
        }
        if hooks.is_empty() {
            return None;
        }
        let mut output = String::from("{\n");
        for (index, hook) in hooks.iter().enumerate() {
            if index != 0 {
                output.push('\n');
            }
            output.push_str(&indent_source(hook, 4));
        }
        output.push_str("\n}");
        Some(output)
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_property(property: &ClassProperty, methods: &[ClassMethod]) -> Option<String> {
        let attributes = render_attribute_lines(&property.attributes)?;
        let mut parts = Vec::new();
        if property.is_final {
            parts.push("final".to_string());
        }
        parts.push(visibility_source(property.visibility).to_string());
        if let Some(set_visibility) = property.set_visibility {
            parts.push(format!("{}(set)", visibility_source(set_visibility)));
        }
        if property.is_static {
            parts.push("static".to_string());
        }
        if property.is_readonly {
            parts.push("readonly".to_string());
        }
        if let Some(hint) = &property.type_hint {
            parts.push(render_type_hint(hint)?);
        }
        let mut declaration = format!("{} ${}", parts.join(" "), property.name);
        if let Some(default) = &property.default {
            declaration.push_str(" = ");
            declaration.push_str(&render(default, 0, false)?);
        }
        if property.has_get_hook || property.has_set_hook {
            declaration.push(' ');
            declaration.push_str(&render_property_hook_list(property, methods)?);
        } else {
            declaration.push(';');
        }
        Some(if attributes.is_empty() {
            declaration
        } else {
            format!("{attributes}\n{declaration}")
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_class_constant(constant: &ClassConstant) -> Option<String> {
        let attributes = render_attribute_lines(&constant.attributes)?;
        let mut declaration = format!("{} const", visibility_source(constant.visibility));
        if let Some(hint) = &constant.type_hint {
            declaration.push(' ');
            declaration.push_str(&render_type_hint(hint)?);
        }
        declaration.push(' ');
        declaration.push_str(&constant.name);
        declaration.push_str(" = ");
        declaration.push_str(&render(&constant.value, 0, false)?);
        declaration.push(';');
        // PHP's assertion AST printer does not retain the final class-constant
        // flag even though ordinary declaration metadata does.
        Some(if attributes.is_empty() {
            declaration
        } else {
            format!("{attributes}\n{declaration}")
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_enum_case(case: &EnumCase) -> Option<String> {
        let attributes = render_attributes(&case.attributes)?;
        let mut declaration = format!("case {}", case.name);
        if let Some(value) = &case.value {
            declaration.push_str(" = ");
            declaration.push_str(&render(value, 0, false)?);
        }
        declaration.push(';');
        Some(if attributes.is_empty() {
            declaration
        } else {
            format!("{attributes}\n{declaration}")
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_method(method: &ClassMethod) -> Option<String> {
        if method.name.contains("::") || !method.generic_params.is_empty() {
            return None;
        }
        let attributes = render_attribute_lines(&method.attributes)?;
        let mut params = String::new();
        for (index, parameter) in method.params.iter().enumerate() {
            if index != 0 {
                params.push_str(", ");
            }
            params.push_str(&render_parameter(parameter)?);
        }
        let mut modifiers = vec![visibility_source(method.visibility)];
        if method.is_final {
            modifiers.push("final");
        }
        if method.is_abstract {
            modifiers.push("abstract");
        }
        if method.is_static {
            modifiers.push("static");
        }
        let reference = if method.returns_by_ref { "&" } else { "" };
        let return_type = match &method.return_type {
            Some(hint) => format!(": {}", render_type_hint(hint)?),
            None => String::new(),
        };
        let declaration = if method.is_abstract || !method.has_body {
            format!(
                "{} function {reference}{}({params}){return_type};",
                modifiers.join(" "),
                method.name,
            )
        } else {
            format!(
                "{} function {reference}{}({params}){return_type} {{\n{}}}",
                modifiers.join(" "),
                method.name,
                render_block(&method.body)?,
            )
        };
        Some(if attributes.is_empty() {
            declaration
        } else {
            format!("{attributes}\n{declaration}")
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_enum_body(cases: &[EnumCase], methods: &[ClassMethod]) -> Option<String> {
        let mut last_case = None;
        for case in cases {
            last_case = Some(last_case.map_or(case.line, |line: usize| line.max(case.line)));
        }
        let mut first_method = None;
        for method in methods {
            first_method =
                Some(first_method.map_or(method.line, |line: usize| line.min(method.line)));
        }
        if last_case
            .zip(first_method)
            .is_some_and(|(last_case, first_method)| first_method < last_case)
        {
            // Separate AST vectors cannot reproduce an interleaved member
            // order. The supported form keeps cases before methods.
            return None;
        }
        let mut members = Vec::with_capacity(cases.len() + methods.len());
        for case in cases {
            members.push(render_enum_case(case)?);
        }
        for method in methods {
            members.push(render_method(method)?);
        }
        let mut body = String::new();
        for (index, member) in members.iter().enumerate() {
            if index != 0 {
                body.push('\n');
            }
            body.push_str(&indent_source(member, 4));
        }
        Some(if body.is_empty() {
            "{\n}".to_string()
        } else if methods.is_empty() {
            format!("{{\n{body}\n}}")
        } else {
            // PHP's assertion AST formatter separates a method body from the
            // enum's closing brace with one empty line.
            format!("{{\n{body}\n\n}}")
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_class_constant_group(constants: &[&ClassConstant]) -> Option<String> {
        if constants.len() == 1 {
            return render_class_constant(constants[0]);
        }
        let first = constants[0];
        let attributes = render_attribute_lines(&first.attributes)?;
        for constant in &constants[1..] {
            if constant.visibility != first.visibility
                || constant.type_hint != first.type_hint
                || render_attribute_lines(&constant.attributes)? != attributes
            {
                return None;
            }
        }
        let mut declaration = format!("{} const", visibility_source(first.visibility));
        if let Some(hint) = &first.type_hint {
            declaration.push(' ');
            declaration.push_str(&render_type_hint(hint)?);
        }
        declaration.push(' ');
        for (index, constant) in constants.iter().enumerate() {
            if index != 0 {
                declaration.push_str(", ");
            }
            declaration.push_str(&constant.name);
            declaration.push_str(" = ");
            declaration.push_str(&render(&constant.value, 0, false)?);
        }
        declaration.push(';');
        Some(if attributes.is_empty() {
            declaration
        } else {
            format!("{attributes}\n{declaration}")
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_property_group(
        properties: &[&ClassProperty],
        methods: &[ClassMethod],
    ) -> Option<String> {
        if properties.len() == 1 {
            return render_property(properties[0], methods);
        }
        let first = properties[0];
        let attributes = render_attribute_lines(&first.attributes)?;
        for property in &properties[1..] {
            if property.visibility != first.visibility
                || property.set_visibility != first.set_visibility
                || property.type_hint != first.type_hint
                || property.is_static != first.is_static
                || property.is_readonly != first.is_readonly
                || property.is_final != first.is_final
                || property.is_abstract != first.is_abstract
                || render_attribute_lines(&property.attributes)? != attributes
                || property.has_get_hook
                || property.has_set_hook
            {
                return None;
            }
        }
        let mut modifiers = Vec::new();
        if first.is_final {
            modifiers.push("final".to_string());
        }
        modifiers.push(visibility_source(first.visibility).to_string());
        if let Some(set_visibility) = first.set_visibility {
            modifiers.push(format!("{}(set)", visibility_source(set_visibility)));
        }
        if first.is_static {
            modifiers.push("static".to_string());
        }
        if first.is_readonly {
            modifiers.push("readonly".to_string());
        }
        if let Some(hint) = &first.type_hint {
            modifiers.push(render_type_hint(hint)?);
        }
        let mut values = String::new();
        for (index, property) in properties.iter().enumerate() {
            if index != 0 {
                values.push_str(", ");
            }
            values.push('$');
            values.push_str(&property.name);
            if let Some(default) = &property.default {
                values.push_str(" = ");
                values.push_str(&render(default, 0, false)?);
            }
        }
        let declaration = format!("{} {values};", modifiers.join(" "));
        Some(if attributes.is_empty() {
            declaration
        } else {
            format!("{attributes}\n{declaration}")
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_trait_uses(
        uses: &[GenericAncestor],
        aliases: &[crate::parser::TraitAlias],
        precedences: &[crate::parser::TraitPrecedence],
    ) -> Option<Vec<String>> {
        for ancestor in uses {
            if !ancestor.arguments.is_empty() {
                return None;
            }
        }
        let mut adapted = Vec::new();
        for ancestor in uses {
            let mut has_adaptation = false;
            for alias in aliases {
                if alias.trait_name.as_deref() == Some(ancestor.name.as_str()) {
                    has_adaptation = true;
                    break;
                }
            }
            if !has_adaptation {
                for rule in precedences {
                    if rule.trait_name == ancestor.name {
                        has_adaptation = true;
                        break;
                    }
                }
            }
            if has_adaptation {
                adapted.push(ancestor.name.clone());
            }
        }
        let mut output = Vec::new();
        if !adapted.is_empty() || !aliases.is_empty() || !precedences.is_empty() {
            let names = if adapted.is_empty() {
                let mut names = Vec::with_capacity(uses.len());
                for ancestor in uses {
                    names.push(ancestor.name.clone());
                }
                names
            } else {
                adapted.clone()
            };
            let mut adaptations = Vec::new();
            for rule in precedences {
                adaptations.push(format!(
                    "{}::{} insteadof {};",
                    rule.trait_name,
                    rule.method,
                    rule.instead_of.join(", ")
                ));
            }
            for alias in aliases {
                let owner = match &alias.trait_name {
                    Some(owner) => format!("{owner}::"),
                    None => String::new(),
                };
                let mut modifiers = Vec::new();
                if let Some(visibility) = alias.visibility {
                    modifiers.push(visibility_source(visibility));
                }
                if alias.is_final {
                    modifiers.push("final");
                }
                if let Some(name) = &alias.alias {
                    modifiers.push(name);
                }
                adaptations.push(format!(
                    "{owner}{} as {};",
                    alias.method,
                    modifiers.join(" ")
                ));
            }
            output.push(format!("use {} {{\n{}\n}}", names.join(", "), {
                let mut rendered = String::new();
                for (index, adaptation) in adaptations.iter().enumerate() {
                    if index != 0 {
                        rendered.push('\n');
                    }
                    rendered.push_str(&indent_source(adaptation, 4));
                }
                rendered
            }));
        }
        let mut plain = Vec::new();
        for ancestor in uses {
            let mut is_adapted = false;
            for name in &adapted {
                if name == &ancestor.name {
                    is_adapted = true;
                    break;
                }
            }
            if !is_adapted {
                plain.push(ancestor.name.clone());
            }
        }
        if !plain.is_empty() {
            output.push(format!("use {};", plain.join(", ")));
        }
        Some(output)
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_class_body(
        properties: &[ClassProperty],
        constants: &[ClassConstant],
        methods: &[ClassMethod],
        uses: &[GenericAncestor],
        aliases: &[crate::parser::TraitAlias],
        precedences: &[crate::parser::TraitPrecedence],
    ) -> Option<String> {
        if !uses.is_empty()
            && (!properties.is_empty() || !constants.is_empty() || has_ordinary_method(methods))
        {
            // Trait-use nodes do not yet carry source lines. Avoid publishing
            // a reordered body when a use statement was interleaved with
            // ordinary members; the targeted all-use body is unambiguous.
            return None;
        }
        let mut members = render_trait_uses(uses, aliases, precedences)?;
        let mut ordered = Vec::new();
        for (index, property) in properties.iter().enumerate() {
            ordered.push((property.line, 0_u8, index));
        }
        for (index, constant) in constants.iter().enumerate() {
            ordered.push((constant.line, 1_u8, index));
        }
        for (index, method) in methods.iter().enumerate() {
            if !method.name.contains("::") {
                ordered.push((method.line, 2_u8, index));
            }
        }
        ordered.sort_unstable();
        for pair in ordered.windows(2) {
            if pair[0].0 == pair[1].0 && pair[0].1 != pair[1].1 {
                return None;
            }
        }
        let mut index = 0;
        while index < ordered.len() {
            let (line, kind, _) = ordered[index];
            let mut end = index + 1;
            while end < ordered.len() && ordered[end].0 == line && ordered[end].1 == kind {
                end += 1;
            }
            match kind {
                0 => {
                    let mut group = Vec::with_capacity(end - index);
                    for (_, _, property) in &ordered[index..end] {
                        group.push(&properties[*property]);
                    }
                    members.push(render_property_group(&group, methods)?);
                }
                1 => {
                    let mut group = Vec::with_capacity(end - index);
                    for (_, _, constant) in &ordered[index..end] {
                        group.push(&constants[*constant]);
                    }
                    members.push(render_class_constant_group(&group)?);
                }
                2 => {
                    for (_, _, method) in &ordered[index..end] {
                        members.push(render_method(&methods[*method])?);
                    }
                }
                _ => unreachable!(),
            }
            index = end;
        }
        let mut body = String::new();
        for (index, member) in members.iter().enumerate() {
            if index != 0 {
                body.push('\n');
                if members[index - 1].contains(" function ") && member.contains(" function ") {
                    body.push('\n');
                }
            }
            body.push_str(&indent_source(member, 4));
        }
        let has_method = has_ordinary_method(methods);
        Some(if body.is_empty() {
            "{\n}".to_string()
        } else if has_method {
            format!("{{\n{body}\n\n}}")
        } else {
            format!("{{\n{body}\n}}")
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn has_ordinary_method(methods: &[ClassMethod]) -> bool {
        for method in methods {
            if !method.name.contains("::") {
                return true;
            }
        }
        false
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn ancestors_have_no_arguments(ancestors: &[GenericAncestor]) -> bool {
        for ancestor in ancestors {
            if !ancestor.arguments.is_empty() {
                return false;
            }
        }
        true
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn optional_ancestor_has_no_arguments(ancestor: Option<&GenericAncestor>) -> bool {
        match ancestor {
            Some(ancestor) => ancestor.arguments.is_empty(),
            None => true,
        }
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn indent_source(source: &str, spaces: usize) -> String {
        let indent = " ".repeat(spaces);
        let mut output = String::with_capacity(source.len() + indent.len());
        for line in source.split_inclusive('\n') {
            if line != "\n" {
                output.push_str(&indent);
            }
            output.push_str(line);
        }
        output
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_expression_sequence(expressions: &[Expr], separator: &str) -> Option<String> {
        let mut output = String::new();
        for (index, expression) in expressions.iter().enumerate() {
            if index != 0 {
                output.push_str(separator);
            }
            output.push_str(&render(expression, 0, false)?);
        }
        Some(output)
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_list_target_sequence(targets: &[ListTarget]) -> Option<String> {
        let mut output = String::new();
        for (index, target) in targets.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            output.push_str(&render_list_target(target)?);
        }
        Some(output)
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_list_target(target: &ListTarget) -> Option<String> {
        Some(match target {
            ListTarget::Variable(name) => format!("${name}"),
            ListTarget::Reference(target) => format!("&{}", render(target, 0, false)?),
            ListTarget::Target(target) => render(target, 0, false)?,
            ListTarget::AppendTarget(target) => format!("{}[]", render(target, 0, false)?),
            ListTarget::Skip => String::new(),
            ListTarget::Nested(targets) => format!("[{}]", render_list_target_sequence(targets)?),
            ListTarget::KeyedVariable { key, var } => {
                format!("{} => ${var}", render(key, 0, false)?)
            }
            ListTarget::KeyedReference { key, target } => format!(
                "{} => &{}",
                render(key, 0, false)?,
                render(target, 0, false)?
            ),
            ListTarget::KeyedTarget { key, target } => format!(
                "{} => {}",
                render(key, 0, false)?,
                render(target, 0, false)?
            ),
            ListTarget::KeyedAppendTarget { key, target } => format!(
                "{} => {}[]",
                render(key, 0, false)?,
                render(target, 0, false)?
            ),
            ListTarget::KeyedNested { key, targets } => format!(
                "{} => [{}]",
                render(key, 0, false)?,
                render_list_target_sequence(targets)?
            ),
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_foreach_target(target: &ForeachTarget) -> Option<String> {
        Some(match target {
            ForeachTarget::Variable(name) => format!("${name}"),
            ForeachTarget::Target(target) => render(target, 0, false)?,
            ForeachTarget::Destructure(targets) => {
                format!("[{}]", render_list_target_sequence(targets)?)
            }
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn compound_operator(op: BinOp) -> Option<&'static str> {
        Some(match op {
            BinOp::Add => "+=",
            BinOp::Sub => "-=",
            BinOp::Mul => "*=",
            BinOp::Div => "/=",
            BinOp::Mod => "%=",
            BinOp::Concat => ".=",
            BinOp::Pow => "**=",
            BinOp::BitwiseAnd => "&=",
            BinOp::BitwiseOr => "|=",
            BinOp::BitwiseXor => "^=",
            BinOp::ShiftLeft => "<<=",
            BinOp::ShiftRight => ">>=",
            _ => return None,
        })
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_control_body(body: &[Stmt]) -> Option<String> {
        Some(format!("{{\n{}}}", render_block(body)?))
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_statement(statement: &Stmt) -> Option<String> {
        match statement {
            Stmt::Noop => Some(String::new()),
            Stmt::Block(body) => render_control_body(body),
            Stmt::Echo { expressions, .. } => Some(format!(
                "echo {};",
                render_expression_sequence(expressions, ", ")?
            )),
            Stmt::Return { expr, .. } => Some(match expr {
                Some(expr) => format!("return {};", render(expr, 0, false)?),
                None => "return;".to_string(),
            }),
            Stmt::ExprStmt(expr) => Some(format!("{};", render(expr, 0, false)?)),
            Stmt::Assign { var, expr } => Some(format!("${var} = {};", render(expr, 0, false)?)),
            Stmt::CoalesceAssign { target, expr } => Some(format!(
                "{} ??= {};",
                render(target, 0, false)?,
                render(expr, 0, false)?
            )),
            Stmt::CompoundAssign { target, op, expr } => Some(format!(
                "{} {} {};",
                render(target, 0, false)?,
                compound_operator(*op)?,
                render(expr, 0, false)?
            )),
            Stmt::ArrayAssign {
                var, index, expr, ..
            } => Some(format!(
                "${var}[{}] = {};",
                render(index, 0, false)?,
                render(expr, 0, false)?
            )),
            Stmt::NestedArrayAssign {
                root,
                indices,
                expr,
                ..
            } => Some(format!(
                "{}{} = {};",
                render(root, 100, false)?,
                {
                    let mut rendered = String::new();
                    for index in indices {
                        rendered.push('[');
                        rendered.push_str(&render(index, 0, false)?);
                        rendered.push(']');
                    }
                    rendered
                },
                render(expr, 0, false)?
            )),
            Stmt::ArrayPush { var, expr, .. } => {
                Some(format!("${var}[] = {};", render(expr, 0, false)?))
            }
            Stmt::ArrayAppend { target, expr } => Some(format!(
                "{}[] = {};",
                render(target, 0, false)?,
                render(expr, 0, false)?
            )),
            Stmt::BindArrayAppendReference { var, target } => {
                Some(format!("${var} = &{}[];", render(target, 0, false)?))
            }
            Stmt::AssignProp {
                object,
                property,
                expr,
                ..
            } => Some(format!(
                "{}->{property} = {};",
                render(object, 100, false)?,
                render(expr, 0, false)?
            )),
            Stmt::AssignStaticProp {
                class_name,
                property,
                expr,
                ..
            } => Some(format!(
                "{class_name}::${property} = {};",
                render(expr, 0, false)?
            )),
            Stmt::AssignObjArrayDim {
                object,
                property,
                index,
                expr,
                ..
            } => Some(format!(
                "{}->{property}[{}] = {};",
                render(object, 100, false)?,
                render(index, 0, false)?,
                render(expr, 0, false)?
            )),
            Stmt::Unset(values) => Some(format!(
                "unset({});",
                render_expression_sequence(values, ", ")?
            )),
            Stmt::Global(targets) => {
                let mut output = String::new();
                for (index, target) in targets.iter().enumerate() {
                    if index != 0 {
                        output.push('\n');
                    }
                    output.push_str("global ");
                    match target {
                        GlobalTarget::Variable(name) => {
                            output.push('$');
                            output.push_str(name);
                        }
                        GlobalTarget::Dynamic(expression) => {
                            output.push_str(&render_dynamic_variable(expression)?);
                        }
                    }
                    output.push(';');
                }
                Some(output)
            }
            Stmt::StaticVar { vars, .. } => {
                let mut output = String::new();
                for (index, (name, value)) in vars.iter().enumerate() {
                    if index != 0 {
                        output.push('\n');
                    }
                    output.push_str("static $");
                    output.push_str(name);
                    if let Some(value) = value {
                        output.push_str(" = ");
                        output.push_str(&render(value, 0, false)?);
                    }
                    output.push(';');
                }
                Some(output)
            }
            Stmt::ListAssign { targets, expr, .. } => Some(format!(
                "[{}] = {};",
                render_list_target_sequence(targets)?,
                render(expr, 0, false)?
            )),
            Stmt::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                let mut output = format!(
                    "if ({}) {}",
                    render(condition, 0, false)?,
                    render_control_body(then_body)?
                );
                if let [nested @ Stmt::If { elseif, .. }] = else_body.as_slice() {
                    let nested = render_statement(nested)?;
                    output.push_str(if *elseif { " else" } else { " else " });
                    output.push_str(&nested);
                } else if !else_body.is_empty() {
                    output.push_str(" else ");
                    output.push_str(&render_control_body(else_body)?);
                }
                Some(output)
            }
            Stmt::While { condition, body } => Some(format!(
                "while ({}) {}",
                render(condition, 0, false)?,
                render_control_body(body)?
            )),
            Stmt::DoWhile { condition, body } => Some(format!(
                "do {} while ({});",
                render_control_body(body)?,
                render(condition, 0, false)?
            )),
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                let mut initialization = String::new();
                for (index, statement) in init.iter().enumerate() {
                    if index != 0 {
                        initialization.push_str(", ");
                    }
                    let rendered = render_statement(statement)?;
                    initialization.push_str(rendered.trim_end_matches(';'));
                }
                Some(format!(
                    "for ({}; {}; {}) {}",
                    initialization,
                    render_expression_sequence(condition, ", ")?,
                    render_expression_sequence(update, ", ")?,
                    render_control_body(body)?
                ))
            }
            Stmt::Foreach {
                array,
                value,
                key,
                by_ref,
                body,
                ..
            } => {
                let value = render_foreach_target(value)?;
                let target = match key {
                    Some(key) => format!(
                        "{} => {}{value}",
                        render_foreach_target(key)?,
                        if *by_ref { "&" } else { "" }
                    ),
                    None => format!("{}{value}", if *by_ref { "&" } else { "" }),
                };
                Some(format!(
                    "foreach ({} as {target}) {}",
                    render(array, 0, false)?,
                    render_control_body(body)?
                ))
            }
            Stmt::Break { level, .. } => Some(match level {
                Some(level) => format!("break {level};"),
                None => "break;".to_string(),
            }),
            Stmt::Continue { level, .. } => Some(match level {
                Some(level) => format!("continue {level};"),
                None => "continue;".to_string(),
            }),
            Stmt::Label(name) => Some(format!("{name}:")),
            Stmt::Goto { name, .. } => Some(format!("goto {name};")),
            Stmt::Switch { expr, cases } => {
                let mut output = format!("switch ({}) {{\n", render(expr, 0, false)?);
                for case in cases {
                    output.push_str("    ");
                    match &case.value {
                        Some(value) => {
                            output.push_str("case ");
                            output.push_str(&render(value, 0, false)?);
                            output.push_str(":\n");
                        }
                        None => output.push_str("default:\n"),
                    }
                    output.push_str(&indent_source(&render_block(&case.body)?, 4));
                }
                output.push('}');
                Some(output)
            }
            Stmt::TryCatch {
                try_body,
                catches,
                finally_body,
            } => {
                let mut output = format!("try {}", render_control_body(try_body)?);
                for catch in catches {
                    output.push_str(" catch (");
                    output.push_str(&catch.types.join("|"));
                    if let Some(variable) = &catch.var {
                        output.push_str(" $");
                        output.push_str(variable);
                    }
                    output.push_str(") ");
                    output.push_str(&render_control_body(&catch.body)?);
                }
                if let Some(finally_body) = finally_body {
                    output.push_str(" finally ");
                    output.push_str(&render_control_body(finally_body)?);
                }
                Some(output)
            }
            Stmt::Throw { expr, .. } => Some(format!("throw {};", render(expr, 0, false)?)),
            Stmt::Declare { directives, body } => {
                let mut rendered_directives = String::new();
                for (index, (directive, value)) in directives.iter().enumerate() {
                    if index != 0 {
                        rendered_directives.push_str(", ");
                    }
                    rendered_directives.push_str(directive);
                    rendered_directives.push_str(" = ");
                    rendered_directives.push_str(&value.to_string());
                }
                Some(match body {
                    Some(body) => {
                        format!(
                            "declare({rendered_directives}) {}",
                            render_control_body(body)?
                        )
                    }
                    None => format!("declare({rendered_directives});"),
                })
            }
            Stmt::Include {
                path,
                is_require,
                is_once,
                ..
            } => Some(format!(
                "{} {};",
                match (*is_require, *is_once) {
                    (false, false) => "include",
                    (false, true) => "include_once",
                    (true, false) => "require",
                    (true, true) => "require_once",
                },
                render(path, 0, false)?
            )),
            Stmt::Const { declarations, .. } => {
                let mut output = String::from("const ");
                for (index, (name, value)) in declarations.iter().enumerate() {
                    if index != 0 {
                        output.push_str(", ");
                    }
                    output.push_str(name);
                    output.push_str(" = ");
                    output.push_str(&render(value, 0, false)?);
                }
                output.push(';');
                Some(output)
            }
            Stmt::Class {
                attributes,
                name,
                parent,
                implements,
                is_abstract,
                is_final,
                is_readonly,
                allow_dynamic_properties,
                properties,
                constants,
                methods,
                uses,
                trait_aliases,
                trait_precedences,
                generic_params,
                ..
            } if optional_ancestor_has_no_arguments(parent.as_ref())
                && ancestors_have_no_arguments(implements)
                && !*allow_dynamic_properties
                && generic_params.is_empty() =>
            {
                let attributes = render_attribute_lines(attributes)?;
                let mut modifiers = Vec::new();
                if *is_abstract {
                    modifiers.push("abstract");
                }
                if *is_final {
                    modifiers.push("final");
                }
                if *is_readonly {
                    modifiers.push("readonly");
                }
                let modifiers = if modifiers.is_empty() {
                    String::new()
                } else {
                    format!("{} ", modifiers.join(" "))
                };
                let parent = match parent {
                    Some(parent) => format!(" extends {}", parent.name),
                    None => String::new(),
                };
                let implements = if implements.is_empty() {
                    String::new()
                } else {
                    format!(" implements {}", render_ancestor_names(implements))
                };
                let body = render_class_body(
                    properties,
                    constants,
                    methods,
                    uses,
                    trait_aliases,
                    trait_precedences,
                )?;
                let declaration = format!("{modifiers}class {name}{parent}{implements} {body}");
                Some(if attributes.is_empty() {
                    declaration
                } else {
                    format!("{attributes}\n{declaration}")
                })
            }
            Stmt::Interface {
                attributes,
                name,
                extends,
                properties,
                constants,
                methods,
                generic_params,
                ..
            } if generic_params.is_empty() && ancestors_have_no_arguments(extends) => {
                let attributes = render_attribute_lines(attributes)?;
                let extends = if extends.is_empty() {
                    String::new()
                } else {
                    format!(" extends {}", render_ancestor_names(extends))
                };
                let body = render_class_body(properties, constants, methods, &[], &[], &[])?;
                let declaration = format!("interface {name}{extends} {body}");
                Some(if attributes.is_empty() {
                    declaration
                } else {
                    format!("{attributes}\n{declaration}")
                })
            }
            Stmt::Trait {
                attributes,
                name,
                properties,
                constants,
                methods,
                uses,
                trait_aliases,
                trait_precedences,
                generic_params,
                ..
            } if generic_params.is_empty() => {
                let attributes = render_attribute_lines(attributes)?;
                let body = render_class_body(
                    properties,
                    constants,
                    methods,
                    uses,
                    trait_aliases,
                    trait_precedences,
                )?;
                let declaration = format!("trait {name} {body}");
                Some(if attributes.is_empty() {
                    declaration
                } else {
                    format!("{attributes}\n{declaration}")
                })
            }
            Stmt::Enum {
                attributes,
                name,
                backing_type,
                implements,
                uses,
                trait_aliases,
                cases,
                properties,
                constants,
                methods,
                ..
            } if implements.is_empty()
                && uses.is_empty()
                && trait_aliases.is_empty()
                && properties.is_empty()
                && constants.is_empty() =>
            {
                let attributes = render_attribute_lines(attributes)?;
                let backing_type = match backing_type {
                    Some(hint) => format!(": {}", render_type_hint(hint)?),
                    None => String::new(),
                };
                let body = render_enum_body(cases, methods)?;
                let declaration = format!("enum {name}{backing_type} {body}");
                Some(if attributes.is_empty() {
                    declaration
                } else {
                    format!("{attributes}\n{declaration}")
                })
            }
            _ => None,
        }
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render_block(statements: &[Stmt]) -> Option<String> {
        let mut output = String::new();
        for statement in statements {
            let rendered = render_statement(statement)?;
            if rendered.is_empty() {
                continue;
            }
            output.push_str(&indent_source(&rendered, 4));
            output.push('\n');
            if matches!(
                statement,
                Stmt::Class { .. }
                    | Stmt::Interface { .. }
                    | Stmt::Trait { .. }
                    | Stmt::Enum { .. }
            ) {
                output.push('\n');
            }
        }
        Some(output)
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_zassertion"))]
    fn render(expr: &Expr, parent_precedence: u8, right_child: bool) -> Option<String> {
        let (text, precedence) = match expr {
            Expr::Integer(value) => (value.to_string(), 100),
            Expr::Float(value) => (render_float(*value), 100),
            Expr::StringLiteral(value) => (quote_string(value), 100),
            Expr::BinaryStringLiteral(value) => (quote_binary_string(value), 100),
            Expr::BacktickLiteral { source, .. } => (format!("`{source}`"), 100),
            Expr::InterpolatedString { value, source, .. } => {
                (render_interpolated_string(source.as_deref(), value)?, 100)
            }
            Expr::Bool(value) => (value.to_string(), 100),
            Expr::Null => ("null".to_string(), 100),
            Expr::Variable { name, .. } => (format!("${name}"), 100),
            Expr::DynamicVariable { name, .. } => (render_dynamic_variable(name)?, 100),
            Expr::MagicConstant { name, .. } => (name.clone(), 100),
            Expr::Constant { name, .. }
                if name.eq_ignore_ascii_case("exit") || name.eq_ignore_ascii_case("die") =>
            {
                ("\\exit()".to_string(), 100)
            }
            Expr::Constant { name, .. } => (name.clone(), 100),
            Expr::CompilerHaltOffsetConstant { name, .. } => (name.clone(), 100),
            Expr::Not(value) => (format!("!{}", render(value, 80, false)?), 80),
            Expr::UnaryPlus(value) => (format!("+{}", render(value, 80, false)?), 80),
            Expr::UnaryMinus(value) => (format!("-{}", render(value, 80, false)?), 80),
            Expr::ErrorSuppress(value) => (format!("@{}", render(value, 80, false)?), 80),
            Expr::BitwiseNot { expr, .. } => (format!("~{}", render(expr, 80, false)?), 80),
            Expr::Cast {
                cast_type, expr, ..
            } => {
                let cast = match cast_type {
                    CastType::Int => "int",
                    CastType::Float => "float",
                    CastType::String => "string",
                    CastType::Bool => "bool",
                    CastType::Array => "array",
                    CastType::Object => "object",
                    CastType::Void => "void",
                };
                (format!("({cast}){}", render(expr, 80, false)?), 80)
            }
            Expr::FirstClassCallable { callable, .. } => {
                let callable = render(callable, 100, false)?;
                (format!("{callable}(...)"), 100)
            }
            Expr::FirstClassFunctionCallable { name, .. } => (format!("{name}(...)"), 100),
            Expr::FunctionCall { name, args, .. } => {
                let name = if name.eq_ignore_ascii_case("exit") || name.eq_ignore_ascii_case("die")
                {
                    "\\exit"
                } else {
                    name
                };
                (format!("{name}({})", render_arguments(args)?), 100)
            }
            Expr::New {
                class_name, args, ..
            } => (
                format!("new {class_name}({})", render_arguments(args)?),
                100,
            ),
            Expr::DynamicNew { class, args, .. } => (
                format!(
                    "new {}({})",
                    render(class, 100, false)?,
                    render_arguments(args)?
                ),
                100,
            ),
            Expr::AnonymousNew {
                attributes,
                args,
                is_readonly,
                allow_dynamic_properties,
                parent,
                implements,
                properties,
                constants,
                methods,
                uses,
                trait_aliases,
                ..
            } if !*allow_dynamic_properties
                && optional_ancestor_has_no_arguments(parent.as_ref())
                && ancestors_have_no_arguments(implements) =>
            {
                let attributes = render_attributes(attributes)?;
                let attributes = if attributes.is_empty() {
                    String::new()
                } else {
                    format!("{attributes} ")
                };
                let arguments = render_arguments(args)?;
                let arguments = if args.is_empty() {
                    None
                } else {
                    Some(format!("({arguments})"))
                };
                let parent = match parent {
                    Some(parent) => format!(" extends {}", parent.name),
                    None => String::new(),
                };
                let implements = if implements.is_empty() {
                    String::new()
                } else {
                    format!(" implements {}", render_ancestor_names(implements))
                };
                let body =
                    render_class_body(properties, constants, methods, uses, trait_aliases, &[])?;
                let readonly = if *is_readonly { "readonly " } else { "" };
                (
                    format!(
                        "new {attributes}{readonly}class{}{}{} {body}",
                        arguments.as_deref().unwrap_or_default(),
                        parent,
                        implements,
                    ),
                    100,
                )
            }
            Expr::Instanceof { expr, class_name } => {
                let expr = render(expr, 30, false)?;
                (format!("{expr} instanceof {class_name}"), 30)
            }
            Expr::DynamicInstanceof { expr, class } => {
                let expr = render(expr, 30, false)?;
                let class = render(class, 30, true)?;
                (format!("{expr} instanceof {class}"), 30)
            }
            Expr::ArrayAccess { array, index, .. } => {
                let array = render(array, 100, false)?;
                let index = render(index, 0, false)?;
                (format!("{array}[{index}]"), 100)
            }
            Expr::ArrayAppendArgument { target, .. } => {
                (format!("{}[]", render(target, 100, false)?), 100)
            }
            Expr::ArrayLiteral(elements) => (render_array(elements)?, 100),
            Expr::PropertyAccess {
                object,
                property,
                nullsafe,
                ..
            } => (
                format!(
                    "{}{}{}",
                    render(object, 100, false)?,
                    if *nullsafe { "?->" } else { "->" },
                    property
                ),
                100,
            ),
            Expr::DynamicPropertyAccess {
                object,
                property,
                nullsafe,
                ..
            } => (
                format!(
                    "{}{}{{{}}}",
                    render(object, 100, false)?,
                    if *nullsafe { "?->" } else { "->" },
                    render(property, 0, false)?
                ),
                100,
            ),
            Expr::MethodCall {
                object,
                method,
                args,
                nullsafe,
                ..
            } => (
                format!(
                    "{}{}{}({})",
                    render(object, 100, false)?,
                    if *nullsafe { "?->" } else { "->" },
                    method,
                    render_arguments(args)?
                ),
                100,
            ),
            Expr::StaticCall {
                class_name,
                method,
                args,
                ..
            } => (
                format!("{class_name}::{method}({})", render_arguments(args)?),
                100,
            ),
            Expr::StaticProperty {
                class_name,
                property,
                ..
            } => (format!("{class_name}::${property}"), 100),
            Expr::DynamicNamedStaticProperty {
                class_name,
                property,
                ..
            } => (
                format!("{class_name}::${{{}}}", render(property, 0, false)?),
                100,
            ),
            Expr::DynamicStaticProperty {
                class, property, ..
            } => (
                format!(
                    "{}::{}",
                    render(class, 100, false)?,
                    match property.as_ref() {
                        Expr::StringLiteral(name) => format!("${name}"),
                        Expr::Variable { name, .. } => format!("${name}"),
                        value => format!("${{{}}}", render(value, 0, false)?),
                    }
                ),
                100,
            ),
            Expr::ClassConstant {
                class_name,
                constant,
                ..
            } => (format!("{class_name}::{constant}"), 100),
            Expr::DynamicNamedClassConstant {
                class_name,
                constant,
            } => (
                format!("{class_name}::{{{}}}", render(constant, 0, false)?),
                100,
            ),
            Expr::DynamicClassConstant {
                class,
                constant,
                dynamic_name,
                ..
            } => (
                format!(
                    "{}::{}",
                    render(class, 100, false)?,
                    if *dynamic_name {
                        format!("{{{}}}", render(constant, 0, false)?)
                    } else {
                        match constant.as_ref() {
                            Expr::StringLiteral(name) => name.clone(),
                            constant => render(constant, 100, false)?,
                        }
                    }
                ),
                100,
            ),
            Expr::DynamicCall {
                callable,
                args,
                method_syntax,
                ..
            } => {
                if *method_syntax
                    && let Expr::ArrayLiteral(elements) = callable.as_ref()
                    && let [owner, member] = elements.as_slice()
                    && owner.key.is_none()
                    && member.key.is_none()
                    && !owner.unpack
                    && !member.unpack
                    && !owner.by_reference
                    && !member.by_reference
                {
                    let (owner, separator, static_syntax) = match &owner.value {
                        Expr::ClassConstant {
                            class_name,
                            constant,
                            ..
                        } if constant.eq_ignore_ascii_case("class") => {
                            (class_name.clone(), "::", true)
                        }
                        owner => (render(owner, 100, false)?, "->", false),
                    };
                    let member = match &member.value {
                        Expr::Variable { name, .. } => format!("${name}"),
                        Expr::DynamicVariable { .. } if static_syntax => {
                            render(&member.value, 100, false)?
                        }
                        member => format!("{{{}}}", render(member, 0, false)?),
                    };
                    return Some(format!(
                        "{owner}{separator}{member}({})",
                        render_arguments(args)?
                    ));
                }
                let callable = if matches!(callable.as_ref(), Expr::Closure { .. }) {
                    format!("({})", render(callable, 0, false)?)
                } else {
                    render(callable, 100, false)?
                };
                (format!("{callable}({})", render_arguments(args)?), 100)
            }
            Expr::DynamicStaticCall {
                class,
                method,
                args,
                ..
            } => (
                format!(
                    "{}::{}({})",
                    render(class, 100, false)?,
                    match method.as_ref() {
                        Expr::StringLiteral(name) => name.clone(),
                        Expr::Variable { name, .. } => format!("${name}"),
                        Expr::DynamicVariable { .. } => render(method, 100, false)?,
                        value => format!("{{{}}}", render(value, 0, false)?),
                    },
                    render_arguments(args)?
                ),
                100,
            ),
            Expr::Closure {
                attributes,
                is_static,
                returns_by_ref,
                params,
                use_vars,
                body,
                return_type,
                generic_params,
                ..
            } if generic_params.is_empty() => {
                let attributes = render_attributes(attributes)?;
                let attributes = if attributes.is_empty() {
                    String::new()
                } else {
                    format!("{attributes} ")
                };
                let mut rendered_params = String::new();
                for (index, parameter) in params.iter().enumerate() {
                    if index != 0 {
                        rendered_params.push_str(", ");
                    }
                    rendered_params.push_str(&render_parameter(parameter)?);
                }
                let return_type = match return_type {
                    Some(hint) => format!(": {}", render_type_hint(hint)?),
                    None => String::new(),
                };
                if body.len() == 1
                    && let Stmt::Return {
                        expr: Some(value),
                        line: 0,
                    } = &body[0]
                {
                    let static_prefix = if *is_static { "static " } else { "" };
                    let reference = if *returns_by_ref { "&" } else { "" };
                    (
                        format!(
                            "{attributes}{static_prefix}fn{reference}({rendered_params}){return_type} => {}",
                            render(value, 0, false)?
                        ),
                        100,
                    )
                } else {
                    let static_prefix = if *is_static { "static " } else { "" };
                    let reference = if *returns_by_ref { " &" } else { " " };
                    let captures = if use_vars.is_empty() {
                        String::new()
                    } else {
                        let mut captures = String::from(" use(");
                        for (index, (name, by_reference, _)) in use_vars.iter().enumerate() {
                            if index != 0 {
                                captures.push_str(", ");
                            }
                            if *by_reference {
                                captures.push('&');
                            }
                            captures.push('$');
                            captures.push_str(name);
                        }
                        captures.push(')');
                        captures
                    };
                    (
                        format!(
                            "{attributes}{static_prefix}function{reference}({rendered_params}){captures}{return_type} {{\n{}}}",
                            render_block(body)?
                        ),
                        100,
                    )
                }
            }
            Expr::Assign { var, expr } => (format!("${var} = {}", render(expr, 5, true)?), 5),
            Expr::AssignTarget { target, expr } => (
                format!("{} = {}", render(target, 5, false)?, render(expr, 5, true)?),
                5,
            ),
            Expr::ArrayAppendAssign {
                target,
                expr,
                by_ref,
            } => (
                format!(
                    "{}[] = {}{}",
                    render(target, 100, false)?,
                    if *by_ref { "&" } else { "" },
                    render(expr, 5, true)?
                ),
                5,
            ),
            Expr::NullCoalesce { left, right } => (
                format!(
                    "{} ?? {}",
                    render(left, 15, false)?,
                    render(right, 15, true)?
                ),
                15,
            ),
            Expr::Elvis { left, right } => (
                format!(
                    "{} ?: {}",
                    render(left, 12, false)?,
                    render(right, 12, true)?
                ),
                12,
            ),
            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
            } => (
                format!(
                    "{} ? {} : {}",
                    render(condition, 12, false)?,
                    render(then_expr, 0, false)?,
                    render(else_expr, 12, true)?
                ),
                12,
            ),
            Expr::Isset(values) => (
                format!("isset({})", render_expression_sequence(values, ", ")?),
                100,
            ),
            Expr::Empty(value) => (format!("empty({})", render(value, 0, false)?), 100),
            Expr::Eval { source, .. } => (format!("eval({})", render(source, 0, false)?), 100),
            Expr::PostInc { name, .. } => (format!("${name}++"), 100),
            Expr::PostDec { name, .. } => (format!("${name}--"), 100),
            Expr::PostIncTarget(target) => (format!("{}++", render(target, 100, false)?), 100),
            Expr::PostDecTarget(target) => (format!("{}--", render(target, 100, false)?), 100),
            Expr::PreInc { name, .. } => (format!("++${name}"), 80),
            Expr::PreDec { name, .. } => (format!("--${name}"), 80),
            Expr::PreIncTarget(target) => (format!("++{}", render(target, 80, false)?), 80),
            Expr::PreDecTarget(target) => (format!("--{}", render(target, 80, false)?), 80),
            Expr::Print(value) => (format!("print {}", render(value, 5, false)?), 5),
            Expr::Yield { value, key } => {
                let value = match value.as_deref() {
                    Some(value) => render(value, 0, false)?,
                    None => String::new(),
                };
                let output = match key.as_deref() {
                    Some(key) => format!("yield {} => {value}", render(key, 0, false)?),
                    None if value.is_empty() => "yield".to_string(),
                    None => format!("yield {value}"),
                };
                (output, 5)
            }
            Expr::YieldFrom { expr, .. } => (format!("yield from {}", render(expr, 5, false)?), 5),
            Expr::Clone {
                expr,
                with_properties,
                ..
            } => {
                let arguments = if let Some(with_properties) = with_properties {
                    format!(
                        "{}, {}",
                        render(expr, 0, false)?,
                        render(with_properties, 0, false)?
                    )
                } else {
                    render(expr, 0, false)?
                };
                (format!("\\clone({arguments})"), 80)
            }
            Expr::CompoundAssignExpression { target, op, expr } => {
                let operator = match op {
                    BinOp::Add => "+=",
                    BinOp::Sub => "-=",
                    BinOp::Mul => "*=",
                    BinOp::Div => "/=",
                    BinOp::Mod => "%=",
                    BinOp::Concat => ".=",
                    BinOp::Pow => "**=",
                    BinOp::BitwiseAnd => "&=",
                    BinOp::BitwiseOr => "|=",
                    BinOp::BitwiseXor => "^=",
                    BinOp::ShiftLeft => "<<=",
                    BinOp::ShiftRight => ">>=",
                    _ => return None,
                };
                let target = render(target, 5, false)?;
                let expr = render(expr, 5, true)?;
                (format!("{target} {operator} {expr}"), 5)
            }
            Expr::Match { expr, arms, .. } => {
                let mut output = format!("match ({}) {{\n", render(expr, 0, false)?);
                for arm in arms {
                    let conditions = match &arm.conditions {
                        Some(conditions) => render_expression_sequence(conditions, ", ")?,
                        None => "default".to_string(),
                    };
                    output.push_str(&format!(
                        "    {conditions} => {},\n",
                        render(&arm.body, 0, false)?
                    ));
                }
                output.push('}');
                (output, 100)
            }
            Expr::BinaryOp {
                op, left, right, ..
            } => {
                let (operator, precedence) = match op {
                    BinOp::Or => ("||", 10),
                    BinOp::And => ("&&", 20),
                    BinOp::BitwiseOr => ("|", 25),
                    BinOp::BitwiseXor => ("^", 26),
                    BinOp::BitwiseAnd => ("&", 27),
                    BinOp::Equal => ("==", 30),
                    BinOp::NotEqual => ("!=", 30),
                    BinOp::Identical => ("===", 30),
                    BinOp::NotIdentical => ("!==", 30),
                    BinOp::Less => ("<", 30),
                    BinOp::LessEqual => ("<=", 30),
                    BinOp::Greater => (">", 30),
                    BinOp::GreaterEqual => (">=", 30),
                    BinOp::Concat => (".", 50),
                    BinOp::ShiftLeft => ("<<", 55),
                    BinOp::ShiftRight => (">>", 55),
                    BinOp::Add => ("+", 60),
                    BinOp::Sub => ("-", 60),
                    BinOp::Mul => ("*", 70),
                    BinOp::Div => ("/", 70),
                    BinOp::Mod => ("%", 70),
                    BinOp::Pow => ("**", 80),
                    _ => return None,
                };
                let right_associative = matches!(op, BinOp::Pow);
                let left = render(left, precedence, right_associative)?;
                let right = render(right, precedence, !right_associative)?;
                (format!("{left} {operator} {right}"), precedence)
            }
            Expr::Pipe {
                input, callable, ..
            } => {
                let precedence = 40;
                let input = render(input, precedence, false)?;
                let callable = if matches!(callable.as_ref(), Expr::Closure { .. }) {
                    format!("({})", render(callable, 0, false)?)
                } else {
                    render(callable, precedence, true)?
                };
                (format!("{input} |> {callable}"), precedence)
            }
            _ => return None,
        };
        if precedence < parent_precedence || (right_child && precedence == parent_precedence) {
            Some(format!("({text})"))
        } else {
            Some(text)
        }
    }
    let expression = render(expr, 0, false)?;
    Some(format!("assert({expression})"))
}
