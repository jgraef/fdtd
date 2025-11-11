# Computational Physics

This is app is work-in-progress. It's intend is to model electromagnetic behavior. Initially we implemented [FDTD][1], but are now working on [FEEC][2] too. These methods are not limited to electromagnetic modeling though, so we might also model other things as well.

# TODO

 - clicking objects: left-click selects, right-click opens context menu
 - manipulating objects: movement, other properties in window
 - camera (and light) config ui
 - persist settings like `CameraConfig`, `CameraLightFilters` (global or per project?)
 - render wiremesh without the `POLYGON_MODE_LINE` feature. Use `PrimitiveTopology::LineList` instead.
   Just need to adjust the number of vertices to `2*n` (2 vertices per line, 3 lines per face, versus just the 3 vertices of a normal triangle), and then pull the right vertices in the shader.
 - transform hierarchy: `GlobalTransform`, `Parent`, propagate.
 - Point lights: Not properly implemented right now, as we only need a point light colocated with the camera, for which we don't need any information that the shader already has.