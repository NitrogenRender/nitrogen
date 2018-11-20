# Nitrogen

Nitrogen is a rendering engine written in Rust, mainly building on top of [`gfx-hal`](https://github.com/gfx-rs/gfx).

**The project is still under development and the API not considered stable!**


## Building

Currently nitrogen is not packaged on crates.io, so if you want to build a project with it a
git dependency has to be used.

### Running the example

Once you have the repository cloned, running

```
cargo run
```

### Including in your project

Add this to the `dependencies` section of your `Cargo.toml`:

```toml
nitrogen = { git = "https://github.com/NitrogenRender/nitrogen" }
```

Then build as usual.


## API Crash-course

**Nitrogen's API was designed with exposing a C API in mind, so it's not as Rust-y as it could be.**

The main piece of nitrogen that the programmer will interface with, is the *`Context`*.

The context contains multiple sub-"contexts", but most methods are duplicated in the context for ease of use.

Many calls will create resources or modify them, but nitrogen rarely ever returns the actual data directly from the calls.
Instead, *handles* are used as much as possible.

If your application intends to present rendered results to a window, the `Context::add_display` method can be used to attach
a `winit` (or alternatively X11) window to the context. This call returns a handle, which can later be used to present images to that window.

After a context was created, images, samplers, buffers and vertex descriptions can be created.

### Enter: the render graph

#### Setup

In an application there will most likely be one or more *render graphs*. A render graph is a list of *render passes*,
which create resources and read or consume resources created by other passes (or provided externally via *Materials*).

In order to construct a graph, it first has to be created from the context.

```rust
let graph = ctx.graph_create();
```

Then passes can be added to the graph.

To add a pass, some information about the pass has to be supplied, as well as the pass *implementation*.

A pass implementation has to implement two functions: `setup` and `execute`.

Inside the `setup` method, a `GraphBuilder` is populated with usage data about the pass. For example:
 - which images or buffers are *created*
 - which images or buffers are *written* to, or *read*
 - which images or buffers are *moved*

Both images and buffers are ***resources***. All resources are *named*.
All resources have fixed "life stages"

 1) The resource is created, bringing the resource name into "scope". The pass that creates a resource can write to it.
 2) After the resource has been created, all other passes can only perform read-only operations, such as reading or copying.
 3) After all reading operations have been done, the resource either dies or will be **moved**.
    
    When moving, a new name is given to the resource and it can be written to again.

Those stages cause a resource name to only refer to *one specific* state of the resource.
This simplifies dependency resolution as well as allowing automatic resource aliasing.

With the `PassImpl` implementation and general information about the pass (shader source, used materials etc)
it can be added to the graph

```rust
ctx.graph_add_pass(graph, "PassName", info, Box::new(pass_impl));
```

Graphs can also have *output resources*, which can be used for things like passing resources between graphs or outputting
images to a screen.

```rust
ctx.graph_add_output(graph, "ResourceName");
```

#### Execution

In order to execute a graph it first has to be *compiled*. When compiling the graph, unneeded passes are culled, order of execution is infered
and information about the usage of resource is gathered.

A compiled graph will insert its current configuration into a cache, so compiling only happens when a new combination is encountered.

To compile a graph, use 

```rust
let res = ctx.graph_compile(graph);
```

During compilation, a number of errors can occur, such as when invalid resources are referenced.

To execute a graph that has ben compiled previously, use

```rust
let res = ctx.render_graph(graph, &execution_context);
```

The second argument is the `ExecutionContext`, which, at the time of writing, only supports setting a *reference size*.
Passes can create images that have a relative size to the reference size. In practice that size will probably be the window size.

The output from the `render_graph` method is a `ExecutionResources` value, which holds any resources marked as a graph output.

If there is only one image output, the `ExecutionResources` value can be used in  the call

```rust
ctx.display_present(display, &res);
```

to display the output image on the window.


## TODOs

 - improve synchronization (multiple frames "in-flight", better synchronization between passes)
 - cycle detection of render passes
 - change execution list generation to put early passes even earlier so less time is spent waiting for fences
 - use SPIR-V reflection to have more fine resource usage inference
 - use SPIR-V reflection to verify materials work with certain shaders
 - write documentation 
 
## License

nitrogen and all accompanying files, unless stated otherwise, are released under the Mozilla Public License 2.0.

All new contributions are expected to be under the MPL 2.0 as well.


