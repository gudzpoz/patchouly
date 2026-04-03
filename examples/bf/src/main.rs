#![feature(rust_preserve_none_cc)]

patchouly::include_stencils!();

use std::{
    env::args,
    io::{self, Read, stdin},
    vec,
};

use example_commons::{InputFn, OutputFn};
use patchouly::{
    PatchError, Program,
    managed::{JumpScope, PatchFunctionBuilder},
};
use stencils::*;

fn main() -> Result<(), io::Error> {
    let mut args = args();
    let name = args.next().unwrap_or_else(|| "cargo run --".to_string());
    if args.len() == 0 {
        eprintln!("Usage: {} <brainfuck_file(s)> ...", name);
        std::process::exit(1);
    }
    let mut opts = CompileOptions {
        tape_size: 128 * 1024,
        ..Default::default()
    };
    while let Some(arg) = args.next() {
        if arg == "--debug" {
            opts.debug = true;
        } else if arg == "--sat" {
            opts.saturating = true;
        } else if arg == "--tape" {
            opts.tape_size = args
                .next()
                .expect("--tape <size>")
                .parse()
                .expect("invalid tape size");
        } else {
            let mut code = String::new();
            if arg == "--" {
                code = args.next().unwrap_or_default();
            } else {
                let mut f = std::fs::File::open(&arg)?;
                code.reserve(f.metadata()?.len() as usize);
                f.read_to_string(&mut code)?;
            }
            compile_and_run(&code, opts).unwrap();
        };
    }
    Ok(())
}

type BFFunction = extern "rust-preserve-none" fn(&mut (), usize, usize, usize) -> usize;
#[derive(Default, Clone, Copy)]
struct CompileOptions {
    debug: bool,
    saturating: bool,
    tape_size: usize,
}

fn compile(code: &str, opts: CompileOptions) -> Result<(Program, BFFunction), PatchError> {
    let bf = BF::parse(code.as_bytes());

    let mut builder = PatchFunctionBuilder::new(&BF_STENCIL_LIBRARY);

    let input_fn = &[InputFn(input).into()];
    let print_fn = &[OutputFn(print).into()];

    let mut blocks_store = vec![];
    bf.visit(&mut |op| -> Result<(), PatchError> {
        match op {
            BfVisitOp::Leaf(_ops) => {}
            BfVisitOp::IntoLoop | BfVisitOp::OutOfLoop => {
                blocks_store.push(builder.create_block());
            }
        }
        Ok(())
    })?;
    let mut blocks = &blocks_store[..];
    let panic = builder.create_block();

    let mut entry = builder.switch_to_entry()?;
    let base_ptr = entry.new_param()?;
    let len = entry.new_param()?;
    let index = entry.new_param()?;
    let params = &[base_ptr, len, index];

    let mut b = entry;
    let mut loop_stack = vec![];
    bf.visit(&mut |op| -> Result<(), PatchError> {
        match op {
            BfVisitOp::Leaf(ops) => {
                let mut movement = 0;
                for op in ops {
                    match op {
                        Op::IncDec(0) | Op::LeftRight(0) => {}

                        Op::IncDec(1) => b.emit(&BF_ADD1, params, &[], &[])?,
                        Op::IncDec(-1) => b.emit(&BF_SUB1, params, &[], &[])?,
                        Op::IncDec(n) => b.emit(
                            if *n > 0 { &BF_ADDN } else { &BF_SUBN },
                            params,
                            &[],
                            &[(*n as isize).unsigned_abs()],
                        )?,

                        Op::LeftRight(1) if opts.saturating => {
                            b.emit(&BF_RIGHT1_SAT, &[index], &[index], &[opts.tape_size - 1])?
                        }
                        Op::LeftRight(1) => {
                            movement += 1;
                            b.emit(&BF_RIGHT1, &[index], &[index], &[])?
                        }
                        Op::LeftRight(-1) if opts.saturating => {
                            b.emit(&BF_LEFT1_SAT, &[index], &[index], &[])?
                        }
                        Op::LeftRight(-1) => {
                            movement -= 1;
                            b.emit(&BF_LEFT1, &[index], &[index], &[])?
                        }
                        Op::LeftRight(n) if opts.saturating => {
                            let n = *n;
                            if n > 0 {
                                b.emit(
                                    &BF_RIGHTN_SAT,
                                    &[index],
                                    &[index],
                                    &[n as usize, opts.tape_size - 1],
                                )?
                            } else {
                                b.emit(
                                    &BF_LEFTN_SAT,
                                    &[index],
                                    &[index],
                                    &[n.unsigned_abs() as usize],
                                )?
                            }
                        }
                        Op::LeftRight(n) => {
                            movement += *n;
                            b.emit(
                                if *n > 0 { &BF_RIGHTN } else { &BF_LEFTN },
                                &[index],
                                &[index],
                                &[n.unsigned_abs() as usize],
                            )?
                        }

                        Op::In => b.emit(&BF_READ, params, &[], input_fn)?,
                        Op::Out => b.emit(&BF_PRINT, params, &[], print_fn)?,

                        Op::SetZero => b.emit(&BF_SET_0, params, &[], &[])?,
                        Op::AddTo(n) => {
                            movement += n;
                            b.emit(&BF_ADD_TO, params, &[], &[*n as isize as usize])?;
                        }
                    }
                    if movement.abs() > 1024 && !opts.saturating {
                        b.branch(
                            &BF_CHECK,
                            params,
                            &[],
                            &[],
                            &[JumpScope::Next, JumpScope::Same(panic)],
                        )?;
                        movement = 0;
                    }
                }
            }
            BfVisitOp::IntoLoop => {
                let next = blocks[0];
                let tail = blocks[blocks.len() - 1];
                blocks = &blocks[1..blocks.len() - 1];
                b.end_branch(&BF_JMP, &[], &[], &[], &[JumpScope::Same(next)], next)?;
                loop_stack.push((b.id(), tail));
                if opts.saturating {
                    b.branch(
                        &BF_IF_ZERO_UNCHECKED,
                        params,
                        &[],
                        &[],
                        &[JumpScope::Same(tail), JumpScope::Next],
                    )?;
                } else {
                    b.branch(
                        &BF_IF_ZERO,
                        params,
                        &[],
                        &[],
                        &[
                            JumpScope::Same(tail),
                            JumpScope::Next,
                            JumpScope::Same(panic),
                        ],
                    )?;
                }
            }
            BfVisitOp::OutOfLoop => {
                let (start, tail) = loop_stack.pop().expect("pre-allocated");
                if opts.saturating {
                    b.end_branch(
                        &BF_IF_ZERO_UNCHECKED,
                        params,
                        &[],
                        &[],
                        &[JumpScope::Same(tail), JumpScope::Same(start)],
                        tail,
                    )?;
                } else {
                    b.end_branch(
                        &BF_IF_ZERO,
                        params,
                        &[],
                        &[],
                        &[
                            JumpScope::Same(tail),
                            JumpScope::Same(start),
                            JumpScope::Same(panic),
                        ],
                        tail,
                    )?;
                }
            }
        }
        Ok(())
    })?;
    b.branch(
        &BF_CHECK,
        params,
        &[],
        &[],
        &[JumpScope::Next, JumpScope::Same(panic)],
    )?;
    b.ret(&BF_RET, &[], &[0])?;

    let panic = builder.switch_to_block(panic)?;
    panic.ret(&BF_RET, &[], &[-1isize as usize])?;

    let program = builder.finalize()?;
    assert_eq!(program.stack_slots, 0);
    let run = unsafe { std::mem::transmute::<*const u8, BFFunction>(program.as_ptr()) };

    if opts.debug {
        eprintln!("{:?}", program);
    }

    Ok((program, run))
}

fn compile_and_run(code: &str, opts: CompileOptions) -> Result<(), PatchError> {
    let (_bf, run) = compile(code, opts)?;
    let len = opts.tape_size;
    let (base, _data) = if opts.saturating {
        let exact = vec![0u8; len];
        (exact.as_ptr() as usize, exact)
    } else {
        let padded = vec![0u8; len + 4096 * 2];
        (padded.as_ptr() as usize + 4096, padded)
    };
    let result = run(&mut (), base, len, 0) as isize;
    println!("\nresult: {}", result);
    Ok(())
}

fn print(v: u8) {
    print!("{}", v as char);
}

fn input() -> u8 {
    let mut buf = [0];
    match stdin().read_exact(&mut buf) {
        Ok(_) => buf[0],
        Err(_) => 0,
    }
}

#[derive(Debug, Clone, Copy)]
enum Op {
    IncDec(i32),
    LeftRight(i32),
    In,
    Out,
    /// `[-]`
    SetZero,
    /// `[->>>+<<<]`
    AddTo(i32),
}
#[derive(Debug)]
enum BF {
    Leaf(Vec<Op>),
    Node(Vec<BF>),
    Loop(Box<BF>),
}
impl BF {
    fn parse(code: &[u8]) -> BF {
        fn is_add_to(bf: &BF) -> Option<i32> {
            let BF::Loop(inner) = &bf else {
                return None;
            };
            let BF::Leaf(ops) = inner.as_ref() else {
                return None;
            };
            let Some(
                [
                    Op::IncDec(-1),
                    Op::LeftRight(l),
                    Op::IncDec(1),
                    Op::LeftRight(r),
                ],
            ) = ops[..].as_array()
            else {
                return None;
            };
            if *l != 0 && *l == -*r && l.abs() < 1024 {
                Some(*l)
            } else {
                None
            }
        }
        fn parse_inner(code: &[u8]) -> (BF, &[u8]) {
            let (first, mut code) = parse_leaf(code);
            if code.is_empty() {
                (BF::Leaf(first), code)
            } else {
                let mut nodes = vec![BF::Leaf(first)];
                loop {
                    let node = match code.first() {
                        Some(b'[') => {
                            // Special case checking #1
                            if code.starts_with(b"[-]") {
                                code = &code[3..];
                                BF::Leaf(vec![Op::SetZero])
                            } else {
                                let (mut node, new_code) = parse_loop(code);
                                // Special case checking #2
                                if let Some(offset) = is_add_to(&node) {
                                    node = BF::Leaf(vec![Op::AddTo(offset)])
                                }
                                code = new_code;
                                node
                            }
                        }
                        Some(b']') | None => break,
                        Some(_) => {
                            let (leaf, new_code) = parse_leaf(code);
                            code = new_code;
                            BF::Leaf(leaf)
                        }
                    };
                    if let Some(BF::Leaf(ops)) = nodes.last_mut()
                        && let BF::Leaf(added) = node
                    {
                        ops.extend(added);
                    } else {
                        nodes.push(node);
                    }
                }
                (BF::Node(nodes), code)
            }
        }
        fn parse_leaf(code: &[u8]) -> (Vec<Op>, &[u8]) {
            let end = code
                .iter()
                .position(|b| *b == b'[' || *b == b']')
                .unwrap_or(code.len());
            let mut ops = vec![];
            fn try_merge(ops: &mut Vec<Op>, op: Op) {
                if let Some(last) = ops.last_mut() {
                    match (*last, op) {
                        (Op::IncDec(a), Op::IncDec(b)) => *last = Op::IncDec(a + b),
                        (Op::LeftRight(a), Op::LeftRight(b)) => *last = Op::LeftRight(a + b),
                        _ => ops.push(op),
                    }
                } else {
                    ops.push(op);
                }
            }

            for c in &code[0..end] {
                try_merge(
                    &mut ops,
                    match c {
                        b'+' => Op::IncDec(1),
                        b'-' => Op::IncDec(-1),
                        b'<' => Op::LeftRight(-1),
                        b'>' => Op::LeftRight(1),
                        b'.' => Op::Out,
                        b',' => Op::In,
                        _ => continue,
                    },
                );
            }
            (ops, &code[end..])
        }
        fn parse_loop(code: &[u8]) -> (BF, &[u8]) {
            assert_eq!(code[0], b'[');
            let code = &code[1..];
            let (node, code) = parse_inner(code);
            let code = if code.is_empty() {
                eprintln!("automatically closing a loop: unexpected eof");
                code
            } else {
                &code[1..]
            };
            (BF::Loop(Box::new(node)), code)
        }

        let (node, code) = parse_inner(code);
        if !code.is_empty() {
            eprintln!("trailing code: {}", String::from_utf8_lossy(code));
        }
        node
    }

    fn visit<'a, E, F: FnMut(BfVisitOp<'a>) -> Result<(), E>>(
        &'a self,
        f: &mut F,
    ) -> Result<(), E> {
        match self {
            BF::Leaf(ops) => {
                f(BfVisitOp::Leaf(ops))?;
            }
            BF::Node(nodes) => {
                for node in nodes {
                    node.visit(f)?;
                }
            }
            BF::Loop(node) => {
                f(BfVisitOp::IntoLoop)?;
                node.visit(f)?;
                f(BfVisitOp::OutOfLoop)?;
            }
        }
        Ok(())
    }
}
enum BfVisitOp<'a> {
    Leaf(&'a Vec<Op>),
    IntoLoop,
    OutOfLoop,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run() {
        let (_bf, run) = compile("++++[->++++[->----<]<]", Default::default()).unwrap();
        let data = vec![0u8; 3];
        let result = run(&mut (), data.as_ptr() as usize, data.len(), 0) as isize;
        assert_eq!(result, 0);
        assert_eq!(data, [0, 0, (256 - 64) as u8]);
    }
}
