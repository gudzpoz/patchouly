# Register Allocation in Copy-and-patch

Now that we've covered the basics of copy-and-patch, we can finally start
talking about why it is *very very nice*.

## Stencils come in batches

You see, even ignoring optimizations, the same code snippet, the same
"operation" can compile to vastly different code. For example, `a + 1` might
compile to a simple `inc edi` instruction if the result can be stored in place:
if `a` is not used afterwards and if `edi` fits into the operations that follow:

<iframe width="800px" height="200px" src="https://godbolt.org/e#g:!((g:!((g:!((h:codeEditor,i:(filename:'1',fontScale:14,fontUsePx:'0',j:1,lang:___c,paneName:___C,selection:(endColumn:1,endLineNumber:7,positionColumn:1,positionLineNumber:7,selectionStartColumn:1,selectionStartLineNumber:7,startColumn:1,startLineNumber:7),source:'extern+int+copy_and_patch_next(int+a)%3B%0A%0Aint+inc(int+a)+%7B%0A++++__attribute__((musttail))%0A++++return+copy_and_patch_next(a+%2B+1)%3B%0A%7D%0A'),l:'5',n:'0',o:___C,t:'0')),k:50,l:'4',m:100,n:'0',o:'',s:0,t:'0'),(g:!((h:compiler,i:(compiler:cclang2110,filters:(b:'0',binary:'1',binaryObject:'1',commentOnly:'0',debugCalls:'1',demangle:'0',directives:'0',execute:'1',intel:'0',libraryCode:'0',trim:'1',verboseDemangling:'0'),flagsViewOpen:'1',fontScale:14,fontUsePx:'0',j:1,lang:___c,libs:!(),options:'-O1',overrides:!(),paneName:'+x86-64+clang+21.1.0',selection:(endColumn:1,endLineNumber:1,positionColumn:1,positionLineNumber:1,selectionStartColumn:1,selectionStartLineNumber:1,startColumn:1,startLineNumber:1),source:1),l:'5',n:'0',o:'+x86-64+clang+21.1.0',t:'0')),header:(),k:50,l:'4',n:'0',o:'',s:0,t:'0')),l:'2',n:'0',o:'',t:'0')),version:4"></iframe>

However, if we require the result to be stored somewhere else... then the results will be
vastly different:

<iframe width="800px" height="200px" src="https://godbolt.org/e#g:!((g:!((g:!((h:codeEditor,i:(filename:'1',fontScale:14,fontUsePx:'0',j:1,lang:___c,paneName:___C,selection:(endColumn:1,endLineNumber:7,positionColumn:1,positionLineNumber:7,selectionStartColumn:1,selectionStartLineNumber:7,startColumn:1,startLineNumber:7),source:'extern+int+copy_and_patch_next(int+_,+int+a)%3B%0A%0Aint+inc(int+a,+int+_)+%7B%0A++++__attribute__((musttail))%0A++++return+copy_and_patch_next(_,+a+%2B+1)%3B%0A%7D%0A'),l:'5',n:'0',o:___C,t:'0')),k:50,l:'4',m:100,n:'0',o:'',s:0,t:'0'),(g:!((h:compiler,i:(compiler:cclang2110,filters:(b:'0',binary:'1',binaryObject:'1',commentOnly:'0',debugCalls:'1',demangle:'0',directives:'0',execute:'1',intel:'0',libraryCode:'0',trim:'1',verboseDemangling:'0'),flagsViewOpen:'1',fontScale:14,fontUsePx:'0',j:1,lang:___c,libs:!(),options:'-O1',overrides:!(),paneName:'+x86-64+clang+21.1.0',selection:(endColumn:1,endLineNumber:1,positionColumn:1,positionLineNumber:1,selectionStartColumn:1,selectionStartLineNumber:1,startColumn:1,startLineNumber:1),source:1),l:'5',n:'0',o:'+x86-64+clang+21.1.0',t:'0')),header:(),k:50,l:'4',n:'0',o:'',s:0,t:'0')),l:'2',n:'0',o:'',t:'0')),version:4"></iframe>

And things will be different again if you are so many variables such that `a` is spilled
onto the stack:

<iframe width="800px" height="200px" src="https://godbolt.org/e#g:!((g:!((g:!((h:codeEditor,i:(filename:'1',fontScale:14,fontUsePx:'0',j:1,lang:___c,paneName:___C,selection:(endColumn:55,endLineNumber:5,positionColumn:55,positionLineNumber:5,selectionStartColumn:55,selectionStartLineNumber:5,startColumn:55,startLineNumber:5),source:'extern+int+copy_and_patch_next(int+_1,+int+_2,+int+_3,+int+_4,+int+_5,+int+_6,+int+a)%3B%0A%0Aint+inc(int+_1,+int+_2,+int+_3,+int+_4,+int+_5,+int+_6,+int+a)+%7B%0A++++__attribute__((musttail))%0A++++return+copy_and_patch_next(_1,+_2,+_3,+_4,+_5,+_6,+a+%2B+1)%3B%0A%7D%0A'),l:'5',n:'0',o:___C,t:'0')),k:50,l:'4',m:100,n:'0',o:'',s:0,t:'0'),(g:!((h:compiler,i:(compiler:cclang2110,filters:(b:'0',binary:'1',binaryObject:'1',commentOnly:'0',debugCalls:'1',demangle:'0',directives:'0',execute:'1',intel:'0',libraryCode:'0',trim:'1',verboseDemangling:'0'),flagsViewOpen:'1',fontScale:14,fontUsePx:'0',j:1,lang:___c,libs:!(),options:'-O1',overrides:!(),paneName:'+x86-64+clang+21.1.0',selection:(endColumn:1,endLineNumber:1,positionColumn:1,positionLineNumber:1,selectionStartColumn:1,selectionStartLineNumber:1,startColumn:1,startLineNumber:1),source:1),l:'5',n:'0',o:'+x86-64+clang+21.1.0',t:'0')),header:(),k:50,l:'4',n:'0',o:'',s:0,t:'0')),l:'2',n:'0',o:'',t:'0')),version:4"></iframe>

> And, let's not mention how `-O0` puts everything on the stack:
>
> <iframe width="800px" height="200px" src="https://godbolt.org/e#g:!((g:!((g:!((h:codeEditor,i:(filename:'1',fontScale:14,fontUsePx:'0',j:1,lang:___c,paneName:___C,selection:(endColumn:1,endLineNumber:7,positionColumn:1,positionLineNumber:7,selectionStartColumn:1,selectionStartLineNumber:7,startColumn:1,startLineNumber:7),source:'extern+int+copy_and_patch_next(int+a)%3B%0A%0Aint+inc(int+a)+%7B%0A++++__attribute__((musttail))%0A++++return+copy_and_patch_next(a+%2B+1)%3B%0A%7D%0A'),l:'5',n:'0',o:___C,t:'0')),k:50,l:'4',m:100,n:'0',o:'',s:0,t:'0'),(g:!((h:compiler,i:(compiler:cclang2110,filters:(b:'0',binary:'1',binaryObject:'1',commentOnly:'0',debugCalls:'1',demangle:'0',directives:'0',execute:'1',intel:'0',libraryCode:'0',trim:'1',verboseDemangling:'0'),flagsViewOpen:'1',fontScale:14,fontUsePx:'0',j:1,lang:___c,libs:!(),options:'-O0',overrides:!(),paneName:'+x86-64+clang+21.1.0',selection:(endColumn:1,endLineNumber:1,positionColumn:1,positionLineNumber:1,selectionStartColumn:1,selectionStartLineNumber:1,startColumn:1,startLineNumber:1),source:1),l:'5',n:'0',o:'+x86-64+clang+21.1.0',t:'0')),header:(),k:50,l:'4',n:'0',o:'',s:0,t:'0')),l:'2',n:'0',o:'',t:'0')),version:4"></iframe>

Cases above vary because each of them has a different requirement for the incoming
arguments and outgoing return values. And if you are writing a compiler, then you
will need to not only handle all these cases, but also find ways to put values into
the most suitable registers or stack slots to make the code as efficient as possible.
This is hard, and this can be computationally expensive, and this is where copy-and-patch
comes in.

### Abusing ABI to batch-generate stencils

In the previous chapter, we talked about how concatenating the body of the two functions,
`push_answer` and `pop_return`, produces a compiled *Answer* program:

```x86asm
0000000000000000 <push_answer>:
   0:	bf 2a 00 00 00          mov    $0x2a,%edi
   5:	e9 00 00 00 00          jmp    a <push_answer+0xa>
            6: R_X86_64_PLT32	copy_and_patch_next-0x4

0000000000000000 <pop_return>:
   0:	48 89 f8                mov    %rdi,%rax
   3:	c3                      ret
```

In it, `push_answer` puts `42` into `%rdi`, and `pop_return` returns `%rdi` by copying
it into `%rax`. Why they agree on using `%rdi` and `%rax` is because they are part of
their [ABI]: the first few arguments will use register `X`, `Y`, `Z`, ..., and the return
value will be in `W`, etc., etc.

[ABI]: https://en.wikipedia.org/wiki/Application_binary_interface

And if we change `push_answer` and `pop_return` to use the second argument for value passing,
we will see that they also produce code using a deterministic, fixed set of registers
(`%rsi` and `%rax` now):

<iframe width="800px" height="200px" src="https://godbolt.org/e#g:!((g:!((g:!((h:codeEditor,i:(filename:'1',fontScale:14,fontUsePx:'0',j:1,lang:rust,paneName:Rust,selection:(endColumn:1,endLineNumber:14,positionColumn:1,positionLineNumber:14,selectionStartColumn:1,selectionStartLineNumber:14,startColumn:1,startLineNumber:14),source:'unsafe+extern+%22C%22+%7B%0A++++fn+copy_and_patch_next(_:+usize,+answer:+usize)%3B%0A%7D%0A%0A%23%5Bunsafe(no_mangle)%5D%0Aextern+%22C%22+fn+push_answer()+%7B%0A++++unsafe+%7B+copy_and_patch_next(0,+42)+%7D%3B%0A%7D%0A%0A%23%5Bunsafe(no_mangle)%5D%0Aextern+%22C%22+fn+pop_return(_:+usize,+answer:+usize)+-%3E+usize+%7B%0A++++answer%0A%7D%0A'),l:'5',n:'0',o:Rust,t:'0')),k:50,l:'4',m:100,n:'0',o:'',s:0,t:'0'),(g:!((h:compiler,i:(compiler:r1940,filters:(b:'0',binary:'1',binaryObject:'1',commentOnly:'0',debugCalls:'1',demangle:'0',directives:'0',execute:'1',intel:'0',libraryCode:'0',trim:'1',verboseDemangling:'0'),flagsViewOpen:'1',fontScale:14,fontUsePx:'0',j:1,lang:rust,libs:!(),options:'-O',overrides:!((name:edition,value:'2024')),paneName:'+x86-64+clang+21.1.0',selection:(endColumn:1,endLineNumber:1,positionColumn:1,positionLineNumber:1,selectionStartColumn:1,selectionStartLineNumber:1,startColumn:1,startLineNumber:1),source:1),l:'5',n:'0',o:'+x86-64+clang+21.1.0',t:'0')),header:(),k:50,l:'4',n:'0',o:'',s:0,t:'0')),l:'2',n:'0',o:'',t:'0')),version:4"></iframe>

Now, this actually allows us to access specific registers by using each argument slot,
without even knowing which register is which. And copy-and-patch <del>ab</del>uses this to
generate code for every possible register combination, without a bit of platform-specific
knowledge.

### A short example

Here is an example of how `fn add1(a: usize) -> usize { a + 1 }` can be compiled into
different stencils:

```x86asm
0000000000000000 <add1__1__0>:
   0:   49 8d 45 01             lea    0x1(%r13),%rax
   4:   49 8b 4c 24 10          mov    0x10(%r12),%rcx
   9:   48 c1 e1 03             shl    $0x3,%rcx
   d:   49 03 4c 24 08          add    0x8(%r12),%rcx
  12:   ba 00 00 00 00          mov    $0x0,%edx
                       13: R_X86_64_32 add1__stack0
  17:   48 f7 d2                not    %rdx
  1a:   48 89 04 d1             mov    %rax,(%rcx,%rdx,8)
  1e:   e9 00 00 00 00          jmp    23 <add1__1__0+0x23>
                       1f: R_X86_64_PLT32      copy_and_patch_next-0x4

0000000000000000 <add1__1__1>:
   0:   49 ff c5                inc    %r13
   3:   e9 00 00 00 00          jmp    8 <add1__1__1+0x8>
                        4: R_X86_64_PLT32       copy_and_patch_next-0x4

0000000000000000 <add1__1__2>:
   0:   4d 8d 75 01             lea    0x1(%r13),%r14
   4:   e9 00 00 00 00          jmp    9 <add1__1__2+0x9>
                        5: R_X86_64_PLT32       copy_and_patch_next-0x4
```

`add1__1__0` adds 1 to a register, putting the result onto a "stack";
`add1__1__1` adds 1 in place; and `add1__1__2` adds 1, putting the result
into another register.

To do this, you first encode the `a + 1` operation in ways that are convenient
for your code generation methods, and then generate some functions:

```rust ignore
#[inline(always)]
fn add1(a: usize) -> usize { a + 1 }

pub unsafe fn add1__1__0(stack: &mut Stack, in0: usize) -> () {
    mod imp { /* external symbol declarations */ }
    let stack0 = add1(in0.into());
    stack.set(imp::add1__stack0.as_ptr() as usize, stack0.into());
    become imp::copy_and_patch_next(stack, in0.into());
}

pub unsafe fn add1__1__1(stack: &mut Stack, in0_out0: usize) -> () {
    mod imp { /* external symbol declarations */ }
    let in0_out0 = add1(in0_out0.into());
    become imp::copy_and_patch_next(stack, in0_out0.into());
}

pub unsafe fn add1__1__2(stack: &mut Stack, in0: usize, out0: usize) -> () {
    mod imp { /* external symbol declarations */ }
    let out0 = add1(in0.into());
    become imp::copy_and_patch_next(stack, in0.into(), out0.into());
}

// ...
```

Now, compile the generated code to produce all the stencils you need.

> The code above uses the incomplete, experimental `explicit_tail_calls` feature
> via the `become` keyword. To make more registers available, you might also
> want to know about the ["rust-preserve-none" ABI].
>
> By the way, I think Rust (proc-)macros are quite good for this kind of thing.
> Currently the `patchouly` crate (under the same repository as this book)
> offers something like this:
>
> ```rust
> #![feature(explicit_tail_calls)]
> #![feature(rust_preserve_none_cc)]
> #![allow(incomplete_features)]
> # mod tests {
> use patchouly_macros::stencil;
> # struct Stack();
> # impl Stack {
> #   fn get(&self, i: usize) -> usize { todo!() }
> #   fn set(&self, i: usize, v: usize) -> usize { todo!() }
> # }
>
> #[stencil]
> fn add_const(a: usize, #[hole] c: usize) -> usize {
>   a + c
> }
> #[stencil]
> fn sub(a: usize, b: usize) -> usize {
>   a - b
> }
> # } // mod tests
> ```

["rust-preserve-none" ABI]: https://github.com/rust-lang/rust/issues/151401

[patchouly]: https://github.com/gudzpoz/patchouly

## Register allocation, finally
