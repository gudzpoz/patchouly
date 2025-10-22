# The Simplest Copy-and-patch

In the previous chapter, we designed our *Answer* language, which always returns
42, and JIT-compiled its `PUSH_ANSWER(42) + POP_RETURN` bytecode. However, we
hard-coded x86_64 instructions and constants in our compiler, which makes it
non-portable across platforms and universes. In this chapter, we will look into
copy-and-patch and see how it magically solves these issues for us.

## Do-it-yourself copy-and-patch

I believe copy-and-patch is better explained by actually trying it out by hand.
So let's first try out manual copy-and-patch, and we will delay any introduction
or explanations until later.

### Do-it-yourself: copying

The first step in the copy-and-patch workflow is to write some code snippets,
called **stencils**, in compiled languages like C or Rust. These snippets, when
compiled, will serve as basic building blocks for the JIT runtime to **copy**
from:

```rust
{{#include ./ch1/answer_lang_tail.rs:content}}
```

Let's save the code above in `answer_lang_tail.rs` and compile it with `rustc -O
--emit=obj -C relocation-model=static answer_lang_tail.rs`. Now we can look into
the generated code by calling `objdump --disassemble --reloc
answer_lang_tail.o`, which yields the following:

```x86asm
0000000000000000 <push_answer>:
   0:	bf 2a 00 00 00          mov    $0x2a,%edi
   5:	e9 00 00 00 00          jmp    a <push_answer+0xa>
            6: R_X86_64_PLT32	copy_and_patch_next-0x4

0000000000000000 <pop_return>:
   0:	48 89 f8                mov    %rdi,%rax
   3:	c3                      ret
```

By simply **copying** and concatenating the function bodies of `push_answer` and
`pop_return`, we produce a compiled *Answer* program:

```x86asm
   0:	bf 2a 00 00 00          mov    $0x2a,%edi
   5:	e9 00 00 00 00          jmp    a
   a:	48 89 f8                mov    %rdi,%rax
   d:	c3                      ret
```

Let's try running the code:

```rust
# extern crate memmap2;
# type CFunction = extern "C" fn(...) -> usize;
# fn get_fn(code: &[u8]) -> (CFunction, memmap2::Mmap) {
#     let mut page = memmap2::MmapMut::map_anon(code.len()).expect("mmap failed");
#     page.copy_from_slice(code);
#     let page = page.make_exec().expect("mprotect failed");
#     (unsafe { std::mem::transmute(page.as_ptr()) }, page)
# }
#[cfg(target_arch="x86_64")]
let code = [
    // body of <push_answer>:
    0xbf, 0x2a, 0, 0, 0, // mov edi, 42
    0xe9, 0, 0, 0, 0,    // jmp 0
    // body of <pop_return>:
    0x48, 0x89, 0xf8,    // mov rax, rdi
    0xc3,                // ret
];
let (f, page) = get_fn(&code);
assert_eq!(42, f());
```

Yeah! The test passed and we achieved JIT compilation by merely copying
"stencils".

> Note the above are the outputs I get when running on a x86_64 machine. If you
> are on other architectures, you will need to modify the `code` according to
> what you actually get on your machine.

### Do-it-yourself: patching

But did I not mention that I want to get rid of the constant `42`? To allow
runtime constant **patching**, we need to modify the stencil snippets a bit:

```diff
 unsafe extern "C" {
+    static ANSWER: [u8; 1];
     fn copy_and_patch_next(answer: usize);
 }

 #[unsafe(no_mangle)]
 extern "C" fn push_answer() {
+    let answer = unsafe { ANSWER.as_ptr() } as usize;
+    unsafe { copy_and_patch_next(answer) };
-    unsafe { copy_and_patch_next(42) };
 }
```

And that gives us a slightly different object file output upon compilation:

```diff
 0000000000000000 <push_answer>:
-   0:	bf 2a 00 00 00          mov    $0x2a,%edi
+   0:	bf 00 00 00 00          mov    $0x0,%edi
+			1: R_X86_64_32	ANSWER
    5:	e9 00 00 00 00          jmp    a <push_answer+0xa>
            6: R_X86_64_PLT32	copy_and_patch_next-0x4
```

```rust
# extern crate memmap2;
# type CFunction = extern "C" fn(...) -> usize;
# fn get_fn(code: &[u8]) -> (CFunction, memmap2::Mmap) {
#     let mut page = memmap2::MmapMut::map_anon(code.len()).expect("mmap failed");
#     page.copy_from_slice(code);
#     let page = page.make_exec().expect("mprotect failed");
#     (unsafe { std::mem::transmute(page.as_ptr()) }, page)
# }
#[cfg(target_arch="x86_64")]
let mut code = [
    0xbf,    0, 0, 0, 0,
    0xe9,    0, 0, 0, 0,
    0x48, 0x89, 0xf8,
    0xc3,
];
let (f, page) = get_fn(&code);
assert_eq!(0, f());
```

If we run the code directly, well, it will run fine but return `0`, because now
we will need to **patch** in whatever constant value we want as the return value
now. To do this, we look at the disassembled output and find `1: R_X86_64_32
ANSWER`, and it means that the patch is to be done at offset `1`, sized 32-bit.
Let's patch `42` in and look at the result:

```rust
# extern crate memmap2;
# type CFunction = extern "C" fn(...) -> usize;
# fn get_fn(code: &[u8]) -> (CFunction, memmap2::Mmap) {
#     let mut page = memmap2::MmapMut::map_anon(code.len()).expect("mmap failed");
#     page.copy_from_slice(code);
#     let page = page.make_exec().expect("mprotect failed");
#     (unsafe { std::mem::transmute(page.as_ptr()) }, page)
# }
# #[cfg(target_arch="x86_64")]
# let mut code = [
#     0xbf,    0, 0, 0, 0,
#     0xe9,    0, 0, 0, 0,
#     0x48, 0x89, 0xf8,
#     0xc3,
# ];
# let (f, page) = get_fn(&code);
# assert_eq!(0, f());
code[1..5].copy_from_slice(&42u32.to_ne_bytes());
let (f, page) = get_fn(&code);
assert_eq!(42, f());
```

> If you're on a different architecture, you will probaly see some relocation
> types other than `R_X86_64_32`. For example, on ARM, the compiler might
> produce `R_AARCH64_ADR_PREL_PG_HI21` and `R_AARCH64_ADD_ABS_LO12_NC`, which
> will require [some bit-masking and a little more patching].

[some bit-masking and a little more patching]: https://github.com/llvm/llvm-project/blob/49ba795d1531bbccb93f7f8db6113d5dc4d08e52/lld/ELF/Target.cpp#L1471-L1481

Now it returns 42. And the two examples above just covers the basics of the
**copy**-and-**patch** technique. The rest of this chapter will look into the
two things that make copy-and-patch possible.

> By the way, there are various tutorials on copy-and-patch online already. But
> personally I still think [the talk given by the author of the original paper]
> is the most enlightening and covers some important implementation details.

[the talk given by the author of the original paper]: https://youtu.be/PaQJcBdwG9Y

## Tail call optimization

```x86asm
0000000000000000 <push_answer>:
   0:	bf 2a 00 00 00          mov    $0x2a,%edi
   5:	e9 00 00 00 00          jmp    a <push_answer+0xa>
            6: R_X86_64_PLT32	copy_and_patch_next-0x4
```

Looking at the compiled "push_answer" stencil, we can see at the end of the
function that, instead of returning like "normal functions" do, it jumps to
another function. This is a classic example of tail call optimization (TCO),
that is:

    Calling a function and returning its return value is equivalent to jumping
    to that function and let it return to our caller instead.

Tail call optimization allows us to chain function calls without growing the
stack, and many compilers will try to do this for us with appropriate flags set.
And this is what we see in the assembly: instead of using `call
copy_and_patch_next`, it `jmp` to `copy_and_patch_next`.

This optimization is quite important for copy-and-patch:

1. It allows us to efficiently chain multiple stencil functions together without
   consuming stack space for each concatenated stencil.
2. And thus, it enables the JIT runtime to treat these stencils as reusable
   building blocks that can be seamlessly connected by **copying**.

In the code, there is also this curious `copy_and_patch_next` thing here. But
let's first talk about relocation, or, how linkers know where to inject
pointers.

## Relocation info

The second key component that makes copy-and-patch work is the relocation
information generated by compilers. This info is mainly intended for linkers:
when you refer to things across compilation units (e.g., different C files), it
is then the linkers' job to "patch" in correct addresses for those references.
And relocation info serves exactly this purpose: it tells the linker where to
modify the code in a instruction-agnostic way.

Looking back at our disassembly:

```x86asm
0000000000000000 <push_answer>:
   0:	bf 00 00 00 00          mov    $0x0,%edi
            1: R_X86_64_32	ANSWER
   5:	e9 00 00 00 00          jmp    a <push_answer+0xa>
            6: R_X86_64_PLT32	copy_and_patch_next-0x4
```

We see two relocation entries:

1. `1: R_X86_64_32 ANSWER` - This tells us that at offset 1 in the code, there's
   a 32-bit value that needs to be replaced with the address of the `ANSWER`
   symbol. Since we cast `ANSWER.as_ptr()` to `usize` as our `answer` value, by
   patching in `42` here, we change the return value of the JIT code.

2. `6: R_X86_64_PLT32 copy_and_patch_next-0x4` - This tells us that at offset 6,
   there's a 32-bit relative displacement that needs to be patched to point to
   the `copy_and_patch_next` function.

In order to patch the first entry `ANSWER`, we look into the type of the
relocation: `R_X86_64_32`, which means we should patch in a 32-bit value here in
the form of `S + A`, where `S` is the value `answer` will get, and `A` is
typically `0` for `R_X86_64_32`. So we fill in `42 + 0 = 42u32` here:

```rust
code[1..5].copy_from_slice(&42u32.to_ne_bytes());
```

And that's all you need to inject a 32-bit constant value inline with
copy-and-patch.

The second relocation entry is a bit tricky. What on earth is this
`copy_and_patch_next` here?

### What is this `copy_and_patch_next` thing?

In our example, the `push_answer` function passes control directly to
`copy_and_patch_next` via a relative `jmp` instruction – tail-call-optimized.
Since `copy_and_patch_next` is declared as an `extern` function, a normal linker
would require the existence of such a function so that it can patch its address
in. However, we've done nothing like that in the previous example: we pretty
much just ignored it and never bothered to patch anything there.

Well, theoretically we do need to patch that up. But instead of actually finding
a `copy_and_patch_next` function, we treat the next stencil, that is,
`pop_return`, as `copy_and_patch_next` so that `push_answer` passes control to
`pop_return`. So we actually should patch the relative address to `pop_return`
there. However, since we always concatenate these stencils together, the
relative offset should be `0`, which is what most compilers emit by default. And
this is why we did not patch it above.

Of course, this non-patching approach can be incompatible with some pecular
architectures or compilers, and we might need to handle that by always doing
this relocation. But, as we will see in the rest of this book, it is more
efficient to simply remove these offset-zero jumps, and we will only do
relocation for control flow stencils like `if` or `while`.

### Limitations

By leveraging the power of existing compilers, we are also limited by the way
that they emit code. Notably, for the above procedure to work correctly, we need
some dedicated, magical compiler flags:

- `-C relocation-model=static`: We used this when compiling the Rust stencils
  above. By default, Rust emits PIC (position-independent code, `-C
  relocation-models=pic`) binaries, which use different kinds of relocations.

  If you actually compile the code above without this flags, you might find that
  the relocation on x86_64 will become `R_X86_64_GOTPCREL`. It means that now
  the linker should inject the address to entries in the Global Offset Table
  (GOT), which contains the address of the target data. So:

  - with `-C relocation-model=pic`, you inject addresses of addresses, while
  - with `-C relocation-model=static`, you inject addresses.

  So PIC incurs one more indirect to support more dynamic linkage, but since we
  want to inject addresses (that we interpret as constants) directly, we are to
  use `relocation-model=static`.

- `-C code-model=small/large`: Well, this is new. We did not use this flag in
  the previous examples. It turns out that, because most applications uses less
  than 1 GiB of memory, the compiler and the linker might just assume all symbol
  pointers are within 32-bit or less. This reduces code size, but is ideal for
  us when we want to inject 64-bit integers.

  We can already see this from when we were trying to inject `ANSWER`:

  ```x86asm
     0:	bf 00 00 00 00          mov    $0x0,%edi
            1: R_X86_64_32	ANSWER
  ```

  The compiler produced a `R_X86_64_32` relocation, and we can only inject a
  `u32` there. It can be problematic if we actually hope for a 64-bit value. To
  notify the compiler of the actual ranges of pointer values, we turn to this
  `-C code-model=small/large` flag:

  - With `-C code-model=small`, we can inject a 32-bit integer;
  - with `-C code-model=large`, we can inject 64-bit integers on 64-bit
    platforms now.

  However, if you're to support 32-bit platforms, you simply cannot use pointers
  for 64-bit values (plus there's no 64-bit registers). `-C code-model=large`
  won't help you there, and copy-and-patch is arguably best for langauges in
  which the sizes of data types always match the platform pointers.

  > Note that there is also a `-C code-model=medium` flag, which is available on
  > x86_64 but not on ARM64.

## Summary

In this chapter, we've explored the two key mechanisms that make copy-and-patch
work:

1. **Tail call optimization**: Allows us to chain stencils together efficiently
   without growing the stack.
2. **Relocation information**: Provides the metadata needed to patch constants
   and function addresses at runtime.

By combining these two techniques, we can write our JIT templates in high-level
languages like Rust, compile them to object code, and then copy and patch them
at runtime to generate efficient machine code. Since each platform or
architecture can come with different relocation types, our JIT runtime will need
to actually support a bunch of relocations and that's the last bit of
hard-coding we cannot get rid of. But otherwise, we've mostly delegated the job
of generating code and optimizing assembly code to actual compilers.

In the next chapter, we'll build on these concepts to support register
registration in languages, all the while knowing nothing about platform
registers.
