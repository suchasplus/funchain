# Testing Guide

本文档介绍如何在 `funchain` 项目中运行和管理测试。

## 1. 基础测试命令

使用 Cargo 运行所有测试（包含单元测试、文档测试和集成测试）：

```bash
cargo test
```

或者使用项目中封装好的 Makefile 指令：

```bash
make test
```

## 2. 运行特定测试

### 按名称过滤
只运行包含特定关键字的测试（例如 `base62` 相关测试）：
```bash
cargo test base62
```

### 运行集成测试
只运行 `tests/cli_tests.rs` 中的 CLI 集成测试：
```bash
cargo test --test cli_tests
```

### 运行特定二进制文件的单元测试
```bash
cargo test --bin to62
```

## 3. 测试调试与输出

### 查看打印输出
默认情况下，Cargo 会捕获 `stdout`。若要在测试通过时也查看 `println!` 的输出，请执行：
```bash
cargo test -- --nocapture
```

### 失败时显示回溯
如果测试发生 `panic`，可以设置环境变量查看详细堆栈：
```bash
RUST_BACKTRACE=1 cargo test
```

## 4. 代码覆盖率

项目支持使用 `cargo-llvm-cov` 生成覆盖率报告。

### 生成终端报告
```bash
make coverage
```

### 生成 HTML 报告
```bash
make coverage-html
```
*报告将生成在 `target/llvm-cov/html/index.html`。*

## 5. 项目测试结构

*   **单元测试**: 位于 `src/` 各个文件底部的 `mod tests` 中，主要测试核心逻辑。
*   **文档测试**: 位于 `src/` 的函数注释示例中，确保文档中的示例代码可正确运行。
*   **集成测试**: 位于 `tests/cli_tests.rs`，通过 `assert_cmd` 模拟真实 CLI 调用，测试输入输出。
