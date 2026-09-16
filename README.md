# myzOS

**English** | [简体中文](README.zh-CN.md)

myzOS is a small operating system kernel being built from scratch to explore how operating systems work. It targets 64-bit RISC-V on QEMU's `virt` machine and is written primarily in Rust, with a small amount of RISC-V assembly.

The project aims to connect operating system concepts with their implementation: how a machine enters the kernel, how memory is managed, how programs run, and how data reaches a device or a file.

> **Project status:** myzOS is in the planning stage. v0.1 is the planned first complete educational release; the capabilities listed below have not yet been implemented.

## Overview

myzOS will run as the operating system of a computer emulated by QEMU. QEMU provides the virtual CPU, memory, and devices; myzOS will initialize and manage those resources, provide services to user programs, and offer a simple command-line interface.

The emphasis is on understanding each mechanism and keeping the implementation small enough to read, explain, and debug.

## Architecture

| Component | Design choice |
| --- | --- |
| CPU architecture | 64-bit RISC-V, targeting the RV64GC instruction set |
| Machine platform | QEMU `virt` |
| Kernel design | A monolithic kernel organized into modules |
| Implementation language | Rust, with a small amount of RISC-V assembly |
| Rust environment | `no_std`, using `core` without relying on the standard library `std` |
| Execution model | A single hart (RISC-V hardware thread) for v0.1 |
| User interface | A serial console and a simple shell |

The kernel will separate architecture-specific code and device access from memory management, process management, and file services. Low-level operations that require `unsafe` will be kept within small interfaces with documented assumptions.

## Planned v0.1 scope

v0.1 is intended to form a complete path from machine startup to running user programs and accessing files.

- [ ] **Boot and console:** enter the kernel, establish its initial execution environment, and provide serial output for interaction and diagnostics.
- [ ] **Memory management:** allocate and reclaim physical pages, build Sv39 page tables, and manage separate address spaces for the kernel and user programs.
- [ ] **Traps and interrupts:** handle exceptions, device interrupts, and timer events, with diagnostic information for failures.
- [ ] **User programs and system calls:** execute programs in user mode and provide a small set of system calls for accessing kernel services.
- [ ] **Process management:** support basic process creation, scheduling, exit, and resource reclamation on a single hart.
- [ ] **Block storage and file system:** access a QEMU virtual block device and provide a small file system with directories and basic file creation, reading, and writing.
- [ ] **Shell:** provide a simple command-line interface for launching user programs and performing basic file operations.

The v0.1 completion criterion is a system that can boot in QEMU, start a user-space shell, run simple programs, and create, read, and write files through kernel services.

## Design principles

- **Understandable mechanisms:** favor explicit control flow and clear data structures over premature optimization.
- **Clear boundaries:** keep hardware access, kernel services, and user programs distinct, and document the assumptions between them.
- **Independent implementation:** use existing systems to study concepts and design decisions, then develop and explain the implementation within myzOS.

## Learning references

- [ECNU OSLab 2026](https://gitee.com/christinaaa/ecnu-oslab-2026-task)
- MIT xv6-riscv, particularly `xv6-labs-2020` on the `util` branch

