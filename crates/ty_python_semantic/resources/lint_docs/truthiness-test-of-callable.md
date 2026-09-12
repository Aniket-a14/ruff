## What it does

Detects boolean tests of values typed as `Callable` or as a union made entirely of `Callable` types.

## Why is this bad?

`Callable`-typed variables are nearly always functions in practice, and functions are always truthy.
Testing `if predicate:` therefore usually tests the function object rather than its result;
`if predicate():` is often intended. A callable instance can define its own truthiness, so the test
is suspicious rather than necessarily always true.

## Examples

```py
from collections.abc import Callable


def announce_if_ready(is_ready: Callable[[], bool]) -> None:
    if is_ready:  # error: [truthiness-test-of-callable]
        print("Ready")
```

Call the value to test its result, passing any required arguments:

```py
def announce_if_ready_fixed(is_ready: Callable[[], bool]) -> None:
    if is_ready():  # no diagnostic
        print("Ready")
```

## See also

- `redundant-condition` reports conditions whose inferred types make them always truthy or falsy,
    such as a function that was not called.
- `redundant-condition-strict` reports additional always-truthy or always-falsy conditions when
    enabled.
