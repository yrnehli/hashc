# Hash language modules

A module in `Hash` is equivalent to a namespace that can contain variable definitions, function definitions, type definitions or include other modules.

## Importing

Given the project structure:
```
.
├── lib
│   ├── a.hash
│   ├── b.hash
│   └── sub
│       └── c.hash
└── main.hash
```

Modules in hash allow for a source to be split up into smaller code fragments, allowing for better source code organisation and maintenance.

You can import modules by specifying the path relative to the current path. 

For example, if you wanted to include the modules `a`, `b`, and or `c` within your main file

```rust
// main.hash
a := import("lib/a");
b := import("lib/b");
c := import("lib/sub/c");
```

By doing so, you are placing everything that is defined within each of those modules under
the namespace. 

## Referencing exports 🚧

> **Note**: Currently there is no way to declare if a symbol or type are to be exported or should be contained within the local scope of the module. Of course this is bounded to change and hence why the name `pub` has been reserved for the future.

Furthermore, if the `a` module contained a structure definition like `Point`:

```rust
// a.hash
struct Point = {
    x: u32;
    y: u32;
}
```

Within main, you can create a new `point` by doing the following

```rust
// main.hash
a := import("lib/a");

p1 := a::Point { x=2; y=3 };

print(p1.x); // 2
print(p1.y); // 3
```

So from this example, you use the `::` (namespace access operator) to reference any exports from the module.

Furthermore, what if you wanted to import only a specific definition within a module such as the 'Point' structure from the module `a`.

You can do so by destructuring the definitions into using the syntax as
follows:

```rust
{Point} := import("lib/a");

p1 := Point { x=2; y=3 };
```

In case you have a member of your current module already reserving a name, you
can rename the exported members to your liking:
```rust
{Point: LibPoint} = import("lib/a");

p1 := LibPoint { x=2; y=3 };
```

> **Note**: Naming is entirely up to the developer, there are no restrictions on naming
> except the language naming.