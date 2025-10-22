# Introduction

Most all programmers have dreamt about writing a
[compiler](https://en.wikipedia.org/wiki/Compiler) themselves. And as JIT
(**J**ust-**I**n-**T**ime) compilation gets popular with scripting language
runtimes like [V8](https://chromium.googlesource.com/v8/v8) and
[LuaJIT](https://luajit.org/luajit.html), I bet many of them (which might
include *you*! My dear reader.) are also tempted to implement a JIT runtime for
their own favorite language.

This book will cover the easy part of a tiered JIT engine: a baseline JIT
runtime based on a technique called copy-and-patch.

> Please read on for explanations of all the buzzwords on this page. :)

## Prerequisites

This tutorial assumes that the reader:

- Knows Rust;
- Can read a bit of assembly;
- Knows about various concepts like compilers, linkers, ...

## Acknowledgements

This book (or, blog post series) is very much inspired by [the Copy-and-Patch
tutorials from Alex Miller](https://transactional.blog/copy-and-patch/).
