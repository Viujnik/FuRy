# FuRy

**FuRy** is a cross-platform data ecosystem that eliminates JSON serialization overhead
by mapping database wire protocols directly into **FlatBuffers** binary format using zero-copy principles.

---

## Overview

FuRy provides a unified core that handles database connections, query execution, and binary data transformation.
Instead of parsing JSON and allocating thousands of heavy objects in high-level languages, FuRy delivers raw FlatBuffers
buffers
directly to your application layer with zero intermediate parsing.

---

## Core Objectives

- **Zero-Copy Performance**: Direct mapping from database binary responses to FlatBuffers without intermediate
  serialization/deserialization
- **Multi-Database Support**: Unified interface for relational and analytical databases
- **Memory Safety**: Core ensures safe binary data manipulation across language boundaries
- **Developer Experience**: Automatic schema generation from type annotations — no manual `.fbs` files required
- **Language Agnostic**: Core with native bindings for multiple languages

---

## Architecture

The project follows the **Core Engine Pattern**:

1. **fury-core**: Database connection pools, FlatBuffers runtime compiler, schema registry with offset caching
2. **Language Bindings**: Native bindings with zero-overhead field access for supported languages
3. **CLI Tools**: Code generation utilities for frontend integration

---

## Status: First Blood

The project is in **active development**. Core workspace structure is initialized, and database integration is in
progress.