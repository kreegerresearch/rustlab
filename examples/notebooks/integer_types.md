# Integer Types

rustlab has tagged-width integer types — `int8`, `int16`, `int32`, `int64` and
their unsigned counterparts `uint8` … `uint64` — stored compactly and carrying
their own overflow behaviour. This notebook walks through creating, operating
on, and storing integers.

## Creating integers

Cast a number with `int8` … `uint64`. Real inputs round half away from zero,
and out-of-range values **saturate** to the type's limits by default:

```rustlab
a = int8(200)      % saturates to the int8 maximum (127)
b = int8(2.5)      % rounds half away from zero → 3
c = uint8(-5)      % unsigned floor → 0
disp(class(a))
```

`class(x)` reports the type. Use `intmax` / `intmin` for a class's limits:

```rustlab
disp(intmax("int8"))
disp(intmin("int16"))
disp(isinteger(a))
```

## Radix literals

Hexadecimal (`0x`), binary (`0b`), and octal (`0o`) literals are integers of
the smallest unsigned class that fits the value. Underscores group digits:

```rustlab
mask = 0xdead_beef
disp(mask)
disp(class(mask))
```

## Arithmetic

Same-class arithmetic stays integer and saturates on overflow. Mixing an
integer with a `double` promotes the result to `double`:

```rustlab
s = uint8(200) + uint8(100)   % saturates to 255
p = int32(5) + 2.7            % promotes to double → 7.7
disp(s)
disp(p)
```

Combining two *different* integer classes is an error — cast one explicitly.
The overflow policy is selectable per value; pass `"wrap"` for 2's-complement
wraparound instead of saturation:

```rustlab
w = int8(100, "wrap") + int8(50, "wrap")   % 150 wraps to -106
disp(w)
```

## Integer arrays

Casting a vector or matrix produces a packed integer array. Elementwise
arithmetic and broadcasting keep the class; reductions like `sum` widen to
`double`:

```rustlab
v = int16([10, 20, 30]);
w = v + int16(5);        % broadcast → [15, 25, 35]
disp(w)
disp(sum(v))             % 60 (double)
```

Transpose preserves the class, and integers index like any other array:

```rustlab
m = int8([1, 2; 3, 4]);
disp(transpose(m))
disp(m(2, 1))
```

Integers also flow through the wider builtin surface by widening to `double`
where needed:

```rustlab
disp(sqrt(int32(16)))
disp(sort(int8([3, 1, 2])))
disp(max(int16([5, 9, 2])))
```

## Saving and loading

Integers save to `.npy` with their native NumPy dtype and round-trip as the
same class — including the full `uint64` range:

```rustlab
big = uint64(0xFFFFFFFFFFFFFFFF);
save("ints.npy", int32([1, 2, 3, -4]));
back = load("ints.npy");
disp(class(back))
disp(back)
```

Convert back to double any time with `double`:

```rustlab
disp(double(int16(1000)))
```
