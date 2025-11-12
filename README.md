# Computational Physics

This app is work-in-progress. It's intended to model electromagnetic behavior. Initially we implemented [FDTD][1], but are now working on [FEEC][2] too. These methods are not limited to electromagnetic modeling though, so we might also model other things as well.

# TODO

 - do we need deferred deletion? it complicates things a lot. try to remove it.
  - then delete/cut can take the object out of scene immediately and send it to hades and/or serialize it into clipboard
  - maybe just allow generic on_delete hooks, so e.g. renderer can remove mesh, outline, etc. octtree can remove entity from bvh.

 - clicking objects: left-click selects, right-click opens context menu
 - manipulating objects: movement, other properties in window
 - camera (and light) config ui
 - persist settings like `CameraConfig`, `CameraLightFilters` (global or per project?)
 - render wiremesh without the `POLYGON_MODE_LINE` feature. Use `PrimitiveTopology::LineList` instead.
   Just need to adjust the number of vertices to `2*n` (2 vertices per line, 3 lines per face, versus just the 3 vertices of a normal triangle), and then pull the right vertices in the shader.
 - transform hierarchy: `GlobalTransform`, `Parent`, propagate.
 - Point lights: Not properly implemented right now, as we only need a point light colocated with the camera, for which we don't need any information that the shader already has.
