use std::mem;

mod stencils {
    include!(concat!(env!("OUT_DIR"), "/stencils.rs"));
}

fn main() {
    let mut code = Vec::new();
    let mut relocations = Vec::new();
    let mut offsets = vec![0];
    relocations.push(stencils::emit_add_int1_int2(&mut code, stencils::TargetId(1)));
    offsets.push(code.len());
    stencils::emit_return_int1(&mut code);
    println!("compiled");

    let mut page = memmap2::MmapMut::map_anon(code.len()).expect("memmap failed");
    println!("copied");
    page.copy_from_slice(&code);
    let page = page.make_exec().expect("mprotect failed");
    println!("mprotect");
    let f = unsafe { transmute_fn(page.as_ptr()) };
    println!("running");
    for i in 0..10 {
        let s = f(i, i + 1);
        println!("{} + {} = {}", i, i + 1, s);
    }
    println!("done!");
}

unsafe fn transmute_fn(ptr: *const u8) -> extern "C" fn(...) -> usize {
    unsafe { mem::transmute(ptr) }
}
