#![no_main]

// ANCHOR: focus
unsafe extern "C" {
    static ANSWER: [u8; 1];
    fn copy_and_patch_next(answer: usize);
}

#[unsafe(no_mangle)]
extern "C" fn push_answer() {
    let answer = unsafe { ANSWER.as_ptr() } as usize;
    unsafe { copy_and_patch_next(answer) };
}
// ANCHOR_END: focus

#[unsafe(no_mangle)]
extern "C" fn pop_return(answer: usize) -> usize {
    answer
}
