# Suspicious boolean conditions

This document tests the `truthiness-test-of-callable` and `truthiness-test-of-iterable` rules. The
first warns when a value whose type is `Callable` or a union of callables is tested without being
called. The second warns when a value typed as `Iterable`, `Iterator`, or `Generator` is tested for
truthiness, since an empty generator is still truthy.

The `redundant-condition` and `redundant-condition-strict` rules report conditions that ty can infer
are always truthy or always falsy. The rules tested here flag suspicious conditions even when their
truthiness can vary at runtime.

## Callable values

Testing a `Callable` does not invoke it. We suggest a call when the value takes no arguments:

```py
from collections.abc import Callable

def check(predicate: Callable[[], bool]) -> None:
    if predicate:  # snapshot: truthiness-test-of-callable
        pass
    if predicate():  # no diagnostic
        pass
```

```snapshot
warning[truthiness-test-of-callable]: Suspicious boolean test of a `Callable`
 --> src/mdtest_snippet.py:4:8
  |
4 |     if predicate:  # snapshot: truthiness-test-of-callable
  |        ^^^^^^^^^ Has type `() -> bool`
info: Callable objects are usually functions, and functions are always truthy
help: Did you mean to call this callable?
help: Replace with `predicate()`
  |
3 | def check(predicate: Callable[[], bool]) -> None:
  -     if predicate:  # snapshot: truthiness-test-of-callable
4 +     if predicate():  # snapshot: truthiness-test-of-callable
5 |         pass
  |
note: This is an unsafe fix and may change runtime behavior
```

When arguments are required, the suggested call leaves them for the user to supply:

```py
def check_with_argument(predicate: Callable[[int], bool]) -> None:
    if predicate:  # snapshot: truthiness-test-of-callable
        pass
```

```snapshot
warning[truthiness-test-of-callable]: Suspicious boolean test of a `Callable`
 --> src/mdtest_snippet.py:9:8
  |
9 |     if predicate:  # snapshot: truthiness-test-of-callable
  |        ^^^^^^^^^ Has type `(int, /) -> bool`
info: Callable objects are usually functions, and functions are always truthy
help: Did you mean to call this callable?
help: Replace with `predicate(...)`
   |
8  | def check_with_argument(predicate: Callable[[int], bool]) -> None:
   -     if predicate:  # snapshot: truthiness-test-of-callable
9  +     if predicate(...):  # snapshot: truthiness-test-of-callable
10 |         pass
   |
note: This is a display-only fix and is likely to be incorrect
```

## Unions of callable values

A union of callables still represents callable objects. A union containing `None` may instead be
testing whether the value is present:

```py
from collections.abc import Callable

def check_union(
    predicate: Callable[[int], bool] | Callable[[str], bool],
    optional: Callable[[], bool] | None,
) -> None:
    if predicate:  # error: [truthiness-test-of-callable]
        pass
    if optional:  # no diagnostic
        optional()
```

## Iterable values

An `Iterable` does not promise a length or a meaningful truthiness test. A `Collection` does have a
length and does not include generators:

```py
from collections.abc import Collection, Iterable

def check_items(items: Iterable[int], collection: Collection[int]) -> None:
    if items:  # snapshot: truthiness-test-of-iterable
        pass
    if collection:  # no diagnostic
        pass
```

```snapshot
warning[truthiness-test-of-iterable]: Suspicious boolean test of an `Iterable`
 --> src/mdtest_snippet.py:4:8
  |
4 |     if items:  # snapshot: truthiness-test-of-iterable
  |        ^^^^^ Has type `Iterable[int]`
info: Iterable objects can be generators, and generators are truthy even when empty
help: Test the length of the iterable instead of its truthiness
  |
3 | def check_items(items: Iterable[int], collection: Collection[int]) -> None:
  -     if items:  # snapshot: truthiness-test-of-iterable
4 +     if len(tuple(items)):  # snapshot: truthiness-test-of-iterable
5 |         pass
  |
note: This is a display-only fix and is likely to be incorrect
```

`Iterator` and `Generator` annotations can also describe empty generators:

```py
from collections.abc import Generator, Iterator

def check_iterators(iterator: Iterator[int], generator: Generator[int, None, None]) -> None:
    if iterator:  # error: [truthiness-test-of-iterable]
        pass
    if generator:  # error: [truthiness-test-of-iterable]
        pass
```

## Nested boolean tests

Each tested operand receives the appropriate diagnostic, even when the whole condition remains
ambiguous:

```py
from collections.abc import Callable, Iterable

def check_nested(flag: bool, predicate: Callable[[], bool], items: Iterable[int]) -> None:
    if flag and predicate:  # snapshot: truthiness-test-of-callable
        pass
    if flag and items:  # snapshot: truthiness-test-of-iterable
        pass
```

```snapshot
warning[truthiness-test-of-callable]: Suspicious boolean test of a `Callable`
 --> src/mdtest_snippet.py:4:17
  |
4 |     if flag and predicate:  # snapshot: truthiness-test-of-callable
  |                 ^^^^^^^^^ Has type `() -> bool`
info: Callable objects are usually functions, and functions are always truthy
help: Did you mean to call this callable?
help: Replace with `predicate()`
  |
3 | def check_nested(flag: bool, predicate: Callable[[], bool], items: Iterable[int]) -> None:
  -     if flag and predicate:  # snapshot: truthiness-test-of-callable
4 +     if flag and predicate():  # snapshot: truthiness-test-of-callable
5 |         pass
  |
note: This is an unsafe fix and may change runtime behavior


warning[truthiness-test-of-iterable]: Suspicious boolean test of an `Iterable`
 --> src/mdtest_snippet.py:6:17
  |
6 |     if flag and items:  # snapshot: truthiness-test-of-iterable
  |                 ^^^^^ Has type `Iterable[int]`
info: Iterable objects can be generators, and generators are truthy even when empty
help: Test the length of the iterable instead of its truthiness
  |
5 |         pass
  -     if flag and items:  # snapshot: truthiness-test-of-iterable
6 +     if flag and len(tuple(items)):  # snapshot: truthiness-test-of-iterable
7 |         pass
  |
note: This is a display-only fix and is likely to be incorrect
```
