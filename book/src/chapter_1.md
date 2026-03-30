# The Basics

If you are new to JIT compilation, probably you've been puzzled by all the
buzzwords in the introduction: What exactly is *JIT compilation*? What is this
*tiered JIT runtime* and a *baseline engine*? And what is this *copy-and-patch*
technique? Well, in this chapter, we will talk about that while building the
world's simplest JIT engine.

> If you do know about these terms, then I guess it should be safe to skip this
> chapter if you're in a hurry.

## The *Answer* language

Suppose we have this *Answer* language, that lets any *Answer* programs return
the Ultimate Answer – 42 [^42]:

```console
> [->+<]
42
> {(+⌿⍵)÷≢⍵}⍳10
42
> return 0
42
```

We can implement the *Answer* by compiling the programs into stack-based
bytecode:

```rust
const PUSH_ANSWER: u8 = 1;
const POP_RETURN : u8 = 2;

fn compile_into_bytecode(_program: &str) -> Vec<u8> {
    vec![
        PUSH_ANSWER,
        POP_RETURN,
    ]
}
# assert_eq!(vec![1, 2], compile_into_bytecode(""));
```

There are two bytecode instructions for our ultimate language here:
`PUSH_ANSWER` computes the answer, and pushes it onto the stack; `POP_RETURN`
pops the answer off the stack, and returns it. And we can evaluate them with a
simple interpreter:

```rust
# const PUSH_ANSWER: u8 = 1;
# const POP_RETURN : u8 = 2;
const ANSWER: usize = 42;

fn interpret(bytecode: &[u8]) -> usize {
    let mut stack = vec![];
    let mut i = 0;
    loop {
        match bytecode[i] {
            PUSH_ANSWER => stack.push(ANSWER),
            POP_RETURN  => return stack.pop().unwrap(),
            _ => panic!(),
        }
        i += 1;
    }
}
# fn compile_into_bytecode(_: &str) -> Vec<u8> { vec![1, 2] }
assert_eq!(42, interpret(&compile_into_bytecode("hello world!")));
```

[^42]: Spoiler alert: [Phrases from *The Hitchhiker's Guide to the Galaxy*]

[Phrases from *The Hitchhiker's Guide to the Galaxy*]: https://en.wikipedia.org/wiki/Phrases_from_The_Hitchhiker%27s_Guide_to_the_Galaxy#The_Answer_to_the_Ultimate_Question_of_Life,_the_Universe,_and_Everything_is_42

## Compiling the *Answer* to native code

Our interpreter certainly does not take millions of years to run, but it is
still quite slow – in addition to executing the computation, it has an extra
loop and a huge two-branch `match`, must take care of invalid bytecodes by
panicking and has to do heap allocation for our `Vec`-backed stack. And our
natural answer to that is compiling it to native code:

```rust
# const PUSH_ANSWER: u8 = 1;
# const POP_RETURN : u8 = 2;
#[cfg(target_arch="x86_64")]
fn compile_to_native(bytecode: &[u8]) -> Vec<u8> {
    let mut native_code = vec![];
    for byte in bytecode {
        native_code.extend(match *byte {
            // hard-coded native code for each bytecode
            PUSH_ANSWER => &[
                // mov  eax, 42
                0xb8, 0x2a, 0, 0, 0,
                // push rax
                0x50,
            ][..],
            POP_RETURN  => &[
                // pop rax
                0x58,
                // ret
                0xc3,
            ][..],
            _ => panic!(),
        });
    }
    native_code
}
let code = compile_to_native(&[1, 2]);
assert_eq!(vec![0xb8, 0x2a, 0, 0, 0, 0x50, 0x58, 0xc3], code);
```

> Note that the hard-coded assembly above is for x86_64 only. So, if you're on
> other platforms, the code won't compile at all. Fortunately, the
> copy-and-patch technique, which this book will focus on in the following
> chapters, *is* cross-platform. So you might have to read till the second
> chapter for runnable code.

Now, compiled languages like C will probably emit the compiled native code `b8
2x 00 00 00 50 58 c8` into an executable file or dynamic library for execution
later, that is, the native compilation and execution is typically separated in
different processes. And this is usually referred to as "AOT
(**A**head-**O**f-**T**ime) compilation.

Alternatively, we can also choose to compile the code only when needed, and
execute the compilation product right after compilation. Compared with AOT where
the OS might handle all the executable loading for us, we will need to do a bit
more to run the code directly in our program:

```rust
# const PUSH_ANSWER: u8 = 1;
# const POP_RETURN : u8 = 2;
# #[cfg(target_arch="x86_64")]
# fn compile_to_native(_: &[u8]) -> Vec<u8> { vec![0xb8, 0x2a, 0, 0, 0, 0x50, 0x58, 0xc3] }
let code = compile_to_native(&[PUSH_ANSWER, POP_RETURN]);

type CFunction = extern "C" fn(...) -> usize;
fn get_fn(code: &[u8]) -> (CFunction, memmap2::Mmap) {
    let mut page = memmap2::MmapMut::map_anon(code.len()).expect("mmap failed");
    page.copy_from_slice(code);
    let page = page.make_exec().expect("mprotect failed");
    (unsafe { std::mem::transmute(page.as_ptr()) }, page)
}
let (f, page) = get_fn(&code);
assert_eq!(42, f());
```

Here, we use the [`memmap2`](https://crates.io/crates/memmap2) crate to load our
compiled code into an executable [page] [^mprotect], and then run our code by [transmuting]
the page pointer into a C function pointer. And this whole
compiling-and-then-directly-executing procedure is what we usually refer to as
"**JIT compilation**".

[page]: https://en.wikipedia.org/wiki/Memory_paging
[transmuting]: https://doc.rust-lang.org/std/mem/fn.transmute.html
[`MmapMut::make_exec`]: https://docs.rs/memmap2/latest/memmap2/struct.MmapMut.html#method.make_exec
[`mprotect`]: https://man.archlinux.org/man/mprotect.2

[^mprotect]: We need to explicitly call [`MmapMut::make_exec`] (which might call
    `mprotect` on Linux under the hood, for example) to mark the data as
    executable. This is because modern OS enforces memory protection and one can
    only execute code from executable memory pages. It is also why we cannot
    have JIT compilation on some platforms – they simply forbid normal apps from
    doing things like `make_exec`.

## Optimizing the compiled code

We can quickly notice that currenly the compiled is not optimal: it pushes the
`eax` register onto the stack, and immediately pops the value back. In a better
compiler, we might want to detect patterns like this and produce better code:

```x86asm
;;; instead of these:
        mov eax, 42
        push rax
        pop rax
        ret

;;; we want better code like these:
        mov eax, 42
        ret
```

To do that, we can do *complicated pattern matching* and detect the instruction
pair `PUSH_ANSWER + POP_RETURN`, emitting specialized code for them:

```rust
# const PUSH_ANSWER: u8 = 1;
# const POP_RETURN : u8 = 2;
# #[cfg(target_arch="x86_64")]
# fn compile_to_native(_: &[u8]) -> Vec<u8> { vec![0xb8, 0x2a, 0, 0, 0, 0x50, 0x58, 0xc3] }
# type CFunction = extern "C" fn(...) -> usize;
# fn get_fn(code: &[u8]) -> (CFunction, memmap2::Mmap) {
#     let mut page = memmap2::MmapMut::map_anon(code.len()).expect("mmap failed");
#     page.copy_from_slice(code);
#     let page = page.make_exec().expect("mprotect failed");
#     (unsafe { std::mem::transmute(page.as_ptr()) }, page)
# }
#[cfg(target_arch="x86_64")]
fn compile_to_native_opt(bytecode: &[u8]) -> Vec<u8> {
    if bytecode == &[PUSH_ANSWER, POP_RETURN] {
        // after complicated pattern matching...
        vec![
            // mov eax, 42
            0xb8, 0x2a, 0, 0, 0,
            // ret
            0xc3,
        ]
    } else {
        compile_to_native(bytecode)
    }
}
let code = compile_to_native_opt(&[1, 2]);
let (f, page) = get_fn(&code);
assert_eq!(42, f());
```

However, in actual compilers, there will be a lot more pattern matching like
this as well as tons of flow analysis or other sophisticated optimizations,
which can easily make the compilation time unbearable for JIT runtimes – a
program might spend more time compiling than executing. This can be troublesome
for language runtimes aiming for performance, and one of the solutions that
people came up with is **tiered compilation**, in which:

- The program starts interpreted,
- and the runtime might use a lightweight compiler to produce under-optimized
  native code to make the execution a bit faster while requiring little
  compilation time,
- and when needed, a compiler with more optimizations enabled can be used to
  produce optimized code for *hot* parts of the program.

For example, WebKit uses a four-tier strategy, including:

- an interpreter
- a **baseline** JIT that compilers fastest but yields slowest code,
- a third-tier JIT that compilers slower
- and a fourth-tier JIT that compilers slowest but should produce way better
  code with all kinds of optimizations.

## Baseline JIT with copy-and-patch

The lowest JIT tier in tiered compilation is usually called a **baseline** JIT,
which is more concerned about compilation speed than code performance. For
example, our `compile_to_native` function above can serve as a good baseline JIT
for our *Answer* language: it compiles fast because it only does copies
hard-coded instructions around, and the generated code is faster than
interpreted execution because it does not have a loop or bytecode matching, etc.

But, one problem with our `compile_to_native` function is that it hard-codes too
many things: the code is `x86_64` assembly which is non-portable, and we also
encode `42` directly into the assembly, which might mean that we cannot compute
ultimate answers for other universes!

Getting rid of hard-coded things is hard, and **copy-and-patch** exactly solves
this for us. The gist is: we will trick existing compilation pipelines (GCC,
Clang, LLVM, etc.) to do the hard-coding part for us ahead-of-time; and during
runtime, we still mainly **copy** instructions around with a few **patches**,
making the compilation speed optimal. And it turns out, it is really easy to do
this "tricking", by leveraging tail call optimizations and linkage formats
supported by these compilers, which we go into in the next chapter.
