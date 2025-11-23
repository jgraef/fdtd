# Computational Physics

This app is work-in-progress. It's intended to model electromagnetic behavior. Initially we implemented [FDTD][1], but are now working on [FEEC][2] too. These methods are not limited to electromagnetic modeling though, so we might also model other things as well.

# TODO

 - I think we can get rid of the `CreateProjectionPass` trait. The size of the image buffer/texture can be specified with the target (which it is afaik for the image buffers, so we would specify the texture size when creating the texture channel). Then when in the projection pass the setup (pipeline creation) could be done lazily. Either way we really only need to know the size of the image/texture when setting up the simulation, so it should be passed to `texture_channel()` (this avoids that the renderer has to wait for the size before it can create the texture and the backend can project as soon as it gets a texture from the renderer).
 - PRs for `egui_ltreeview`
 - fdtd: compress material buffer:
  - most points in the lattice contain the same material values
  - turn the current buffer into a buffer that only contains indices to a second buffer that holds the actual data
  - we could then also store the original properties (eps, mu, etc.) for rendering.
 - fdtd-wgpu:
  - render pipeline that renders field values
 - manipulating objects's isometry in scene view
 - pass more config to properties_ui for floats for proper formatting
 - persist settings like `CameraConfig`, `CameraLightFilters` (global or per project?)
 - keybindings
 - rename one outline (selection, shape edges). maybe to "contour"?
 - 2D scene view
 - transform hierarchy: `GlobalTransform`, `Parent`, propagate.
 - Point lights: Not properly implemented right now, as we only need a point light colocated with the camera, for which we don't need any information that the shader already has.
 - use pipeline cache (persistent)

[1]: https://en.wikipedia.org/wiki/Finite-difference_time-domain_method
[2]: https://en.wikipedia.org/wiki/Finite_element_exterior_calculus
