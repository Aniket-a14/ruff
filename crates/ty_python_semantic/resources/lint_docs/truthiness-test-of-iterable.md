## What it does

Detects boolean tests of iterable annotations that permit generators, such as `Iterable`,
`Iterator`, and `Generator`.

## Why is this bad?

An `Iterable` can be a generator, and generators are truthy even when they produce no elements. A
truthiness test therefore does not reliably tell you whether an iterable is empty.

## Examples

```py
from collections.abc import Iterable


def process(items: Iterable[int]) -> None:
    if items:  # error: [truthiness-test-of-iterable]
        print("Received items")
```

An `Iterable` does not promise that `len()` is available. If this function does not need to accept
generators, annotate the parameter as `Collection[int]` instead and test its length:

```py
from collections.abc import Collection


def process_collection(items: Collection[int]) -> None:
    if len(items):  # no diagnostic
        print("Received items")
```

If the function must also accept generators, collect the iterable into a tuple or list before
testing its length. This consumes the generator, so use the collected values afterward:

```py
def process_any(items: Iterable[int]) -> None:
    values = tuple(items)
    if len(values):  # no diagnostic
        print(values)
```

## See also

- `redundant-condition` reports conditions whose inferred types make them always truthy or falsy,
    such as a generator expression.
- `redundant-condition-strict` reports additional always-truthy or always-falsy conditions when
    enabled.
