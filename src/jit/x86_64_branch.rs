//! Final x86 branch relocation and size relaxation.
//!
//! The encoder first emits uniform near branches so lowering can retain simple
//! byte offsets. This pass compacts explicitly admitted forward edges after all
//! logical targets are known and then repatches every affected displacement.

use super::X86BranchFixup;

pub(super) fn relax_short_branches(bytes: &mut Vec<u8>, branches: &[X86BranchFixup]) {
    if !branches.iter().any(|branch| branch.relaxable) {
        return;
    }
    debug_assert!(branches.windows(2).all(|branches| {
        branches[0].instruction + branches[0].near_length <= branches[1].instruction
    }));
    debug_assert!(branches.iter().all(|branch| branch.target.is_some()));

    let mut shortened = vec![false; branches.len()];
    loop {
        let mut changed = false;
        for index in 0..branches.len() {
            let branch = branches[index];
            if !branch.relaxable || shortened[index] {
                continue;
            }
            let target = branch.target.unwrap();
            debug_assert!(target >= branch.instruction + branch.near_length);
            shortened[index] = true;
            let instruction = remap_offset(branch.instruction, branches, &shortened);
            let target = remap_offset(target, branches, &shortened);
            let relative = i64::try_from(target).unwrap() - i64::try_from(instruction + 2).unwrap();
            if i8::try_from(relative).is_ok() {
                changed = true;
            } else {
                shortened[index] = false;
            }
        }
        if !changed {
            break;
        }
    }
    if !shortened.iter().any(|short| *short) {
        return;
    }

    let removed_bytes = branches
        .iter()
        .zip(shortened.iter().copied())
        .filter_map(|(branch, short)| short.then_some(branch.saved_bytes()))
        .sum::<usize>();
    let mut compact = Vec::with_capacity(bytes.len() - removed_bytes);
    let mut cursor = 0;
    let mut shortened_branch = 0;
    while cursor < bytes.len() {
        while shortened_branch < branches.len() && !shortened[shortened_branch] {
            shortened_branch += 1;
        }
        if shortened_branch < branches.len() && branches[shortened_branch].instruction == cursor {
            let branch = branches[shortened_branch];
            compact.extend_from_slice(&[branch.short_opcode, 0]);
            cursor += branch.near_length;
            shortened_branch += 1;
        } else {
            compact.push(bytes[cursor]);
            cursor += 1;
        }
    }

    for (index, branch) in branches.iter().copied().enumerate() {
        let instruction = remap_offset(branch.instruction, branches, &shortened);
        let target = remap_offset(branch.target.unwrap(), branches, &shortened);
        if shortened[index] {
            let relative = i64::try_from(target).unwrap() - i64::try_from(instruction + 2).unwrap();
            compact[instruction + 1] = i8::try_from(relative).unwrap() as u8;
        } else {
            let next_instruction = instruction + branch.near_length;
            let relative =
                i64::try_from(target).unwrap() - i64::try_from(next_instruction).unwrap();
            let relative = i32::try_from(relative)
                .expect("x86 prototype branch exceeds rel32 range after relaxation");
            let displacement = instruction + (branch.displacement - branch.instruction);
            compact[displacement..displacement + std::mem::size_of::<i32>()]
                .copy_from_slice(&relative.to_le_bytes());
        }
    }
    *bytes = compact;
}

fn remap_offset(offset: usize, branches: &[X86BranchFixup], shortened: &[bool]) -> usize {
    offset
        - branches
            .iter()
            .zip(shortened.iter().copied())
            .take_while(|(branch, _)| branch.instruction < offset)
            .filter_map(|(branch, short)| short.then_some(branch.saved_bytes()))
            .sum::<usize>()
}
