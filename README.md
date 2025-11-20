# Computational Physics

This app is work-in-progress. It's intended to model electromagnetic behavior. Initially we implemented [FDTD][1], but are now working on [FEEC][2] too. These methods are not limited to electromagnetic modeling though, so we might also model other things as well.

# TODO

 - render wiremesh without the `POLYGON_MODE_LINE` feature. Use `PrimitiveTopology::LineList` instead.
   Just need to adjust the number of vertices to `2*n` (2 vertices per line, 3 lines per face, versus just the 3 vertices of a normal triangle), and then pull the right vertices in the shader.
 - PRs for `egui_ltreeview`
 - manipulating objects's isometry in scene view
 - pass more config to properties_ui for floats for proper formatting
 - persist settings like `CameraConfig`, `CameraLightFilters` (global or per project?)
 - keybindings
 - rename one outline (selection, shape edges). maybe to "contour"?
 - transform hierarchy: `GlobalTransform`, `Parent`, propagate.
 - Point lights: Not properly implemented right now, as we only need a point light colocated with the camera, for which we don't need any information that the shader already has.
 - use pipeline cache (persistent)

[1]: https://en.wikipedia.org/wiki/Finite-difference_time-domain_method
[2]: https://en.wikipedia.org/wiki/Finite_element_exterior_calculus
