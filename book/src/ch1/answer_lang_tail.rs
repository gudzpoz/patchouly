#![no_main]

// ANCHOR: content
unsafe extern "C" {
    fn copy_and_patch_next(answer: usize);
}

#[unsafe(no_mangle)]
extern "C" fn push_answer() {
    unsafe { copy_and_patch_next(42) };
}

#[unsafe(no_mangle)]
extern "C" fn pop_return(answer: usize) -> usize {
    answer
}
// ANCHOR_END: content
