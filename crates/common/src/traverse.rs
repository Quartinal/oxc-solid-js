use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_semantic::SemanticBuilder;
use oxc_traverse::{traverse_mut, Traverse};

pub fn traverse_program_with_semantic<'a, T>(
    traverser: &mut T,
    allocator: &'a Allocator,
    program: &mut Program<'a>,
) where
    T: Traverse<'a, ()>,
{
    let allocator = allocator as *const Allocator;
    let scoping = SemanticBuilder::new()
        .build(program)
        .semantic
        .into_scoping();

    // SAFETY: The allocator outlives this call and isn't mutated during traversal.
    traverse_mut(traverser, unsafe { &*allocator }, program, scoping, ());
}
