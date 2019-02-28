# nitrogen

[![Build Status](https://travis-ci.org/NitrogenRender/nitrogen.svg?branch=master)](https://travis-ci.org/NitrogenRender/nitrogen)

nitrogen is a rendering engine using "render graphs", using [`gfx-hal`](https://github.com/gfx-rs/gfx).
It lets the user create complex graphics pipelines by creating *graphs* with *passes*.

A pass can create named resources (such as images and buffers), as well as specify that it depends on certain resources in order to execute.

Below is a trimmed down snippet of a graphics pass for deferred lighting. It shows how graph resources are handled by nitrogen.

```rust
struct Lighting;

impl graph::GraphicsPassImpl for Lighting {
    
    fn setup(&mut self, _: &mut graph::Store, builder: &mut graph::GraphBuilder) {
        // create output image "Lit"
        builder.image_create("Lit", graph::ImageCreateInfo { ... });

        // the color attachment binding
        builder.image_write_color("Lit", 0);

        // bind images to descriptors, also use albedo sampler in binding 0.
        // named resources are created by other passes.
        builder.image_read_color("Albedo", 1, Some(0));
        builder.image_read_color("Normal", 2, None);
        builder.image_read_color("Position", 3, None);
        builder.image_read_color("Emission", 4, None);

        builder.enable();
    }

    unsafe fn execute(&self, store: &graph::Store, cmd: &mut graph::GraphicsCommandBuffer) {
        ...
    }
}
```

**Disclaimer**: While usable, the project is still under development and the API might change.

## Goals

 - allow users to build complex rendering pipelines without needing to worry about too many low-level details.
 - existing systems should be understandable even by non-graphics programmers.
 - graphs and passes should be dynamic - *"describe dependencies with data, not code"*.

## Non-Goals

 - "safe/failproof APIs": while safety is desirable, modern graphics APIs are huge and the effort to prove every functionality safe would be enormous. For such an attempt, see [`vulkano`](https://github.com/vulkano-rs/vulkano).
 - pre-built pipelines: nitrogen is a tool to create pipelines, not a collection of them. "Standard" pipelines might be packed into separate crates in the future though.


## Building

Currently nitrogen is not packaged on crates.io, so if you want to build a project with it a
git dependency has to be used.

### Running the example

Once you have the repository cloned, running the following command inside the project directory will launch the `2d-squares` example.

```
cargo run --example 2d-squares
```

More examples can be found in the `examples/` directory.

### Including in your project

To use nitrogen in a cargo project, add the following line to the `dependencies` section of the `Cargo.toml` file:

```toml
nitrogen = { git = "https://github.com/NitrogenRender/nitrogen" }
```

There are a number of feature flags nitrogen exposes, which might need tweaking for your application.

## Documentation

Since nitrogen is not yet released on crates.io, documentation has to be viewed using

```
cargo doc --open
```

(When attempting to view documentation from the nitrogen repository directly, use `cargo doc --open -p nitrogen`)

## TODOs

 - improve synchronization by tracking resource layouts and using barriers
 - more robust and better performing graph compilation
 - use SPIR-V reflection to have more fine resource usage inference
 - use SPIR-V reflection to verify materials work with certain shaders
 - split out some util code into independent crates (`Storage`, `Pool`, etc...)
 
## License

nitrogen and all accompanying files, unless stated otherwise, are released under the Mozilla Public License 2.0.

All new contributions are expected to be under the MPL 2.0 as well.


