# Instructions

## Extracting Compiled Script Bytes

After running `aiken build` in the `me/swap_validator` directory, extract the compiled Plutus script bytes for use in Rust:

```bash
cat plutus.json | python3 -c "
import json, sys
code = json.load(sys.stdin)['validators'][0]['compiledCode']
bytes_list = [f'0x{code[i:i+2]}' for i in range(0, len(code), 2)]
print('pub const SWAP_SCRIPT_BYTES: &[u8] = &[')
print(', '.join(bytes_list))
print('];')
"
```

Paste the output into `src/blockchains/cardano_utils.rs` replacing the placeholder:

```rust
pub const SWAP_SCRIPT_BYTES: &[u8] = &[
    // paste output here
];
```

Then run the Cardano unit tests to verify:

```bash
cargo test cardano
```

# swap_validator

Write validators in the `validators` folder, and supporting functions in the `lib` folder using `.ak` as a file extension.

```aiken
validator my_first_validator {
  spend(_datum: Option<Data>, _redeemer: Data, _output_reference: Data, _context: Data) {
    True
  }
}
```

## Building

```sh
aiken build
```

## Testing

You can write tests in any module using the `test` keyword. For example:

```aiken
use config

test foo() {
  config.network_id + 1 == 42
}
```

To run all tests, simply do:

```sh
aiken check
```

To run only tests matching the string `foo`, do:

```sh
aiken check -m foo
```

## Documentation

If you're writing a library, you might want to generate an HTML documentation for it.

Use:

```sh
aiken docs
```

## Resources

Find more on the [Aiken's user manual](https://aiken-lang.org).

```

```
