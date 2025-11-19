# Computational Physics

This app is work-in-progress. It's intended to model electromagnetic behavior. Initially we implemented [FDTD][1], but are now working on [FEEC][2] too. These methods are not limited to electromagnetic modeling though, so we might also model other things as well.

# TODO

 - add fdtd-cpu backend with rayon for multithreading.
 - simplify `ReadState` and `WriteState`. read/write should also be able to prepare/reuse staging buffers.
 - PRs for `egui_ltreeview`
 - manipulating objects's isometry in scene view
 - pass more config to properties_ui for floats for proper formatting
 - persist settings like `CameraConfig`, `CameraLightFilters` (global or per project?)
 - keybindings
 - rename one outline (selection, shape edges). maybe to "contour"?
 - refactor fdtd to integrate into app better.
  - remove some old stuff which will be used app-wide (like geometry)
  - hide submodules and re-export stuff that should be exposed in the fdtd module namespace.
  - remove origin from simulation (it is not really used)
 - make `Scene::octtree` private.
 - render wiremesh without the `POLYGON_MODE_LINE` feature. Use `PrimitiveTopology::LineList` instead.
   Just need to adjust the number of vertices to `2*n` (2 vertices per line, 3 lines per face, versus just the 3 vertices of a normal triangle), and then pull the right vertices in the shader.
 - transform hierarchy: `GlobalTransform`, `Parent`, propagate.
 - Point lights: Not properly implemented right now, as we only need a point light colocated with the camera, for which we don't need any information that the shader already has.
 - use pipeline cache (persistent)
