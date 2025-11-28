# Computational Physics

This app is work-in-progress. It's intended to model electromagnetic behavior. Initially we implemented [FDTD][1], but are now working on [FEEC][2] too. These methods are not limited to electromagnetic modeling though, so we might also model other things as well.

# TODO

 - hierarchy:
  - fix hierarchy when deleting a parent
 - staging belt:
  - read (gpu to host)
 - read-only properties UI
 - limit how often a running solver projects into textures. done, but needs to be in config somewhere
 - make observers selectable in scene and tree view. the tree view should have a separate category for it. put something into the tree view (and context menus) to show/hide entities. speaking of context menus. i think they're not implemented for the tree view.
 - PRs for `egui_ltreeview`
 - fdtd: compress material buffer:
  - most points in the lattice contain the same material values
  - turn the current buffer into a buffer that only contains indices to a second buffer that holds the actual data
  - we could then also store the original properties (eps, mu, etc.) for rendering.
 - manipulating objects's isometry in scene view
 - pass more config to properties_ui for floats for proper formatting
 - keybindings
 - 2D scene view
 - Point lights: Not properly implemented right now, as we only need a point light colocated with the camera, for which we don't need any information that the shader already has.
 - use pipeline cache (persistent)
 - spatial: add tags whether an entity supports a certain spatial query. then in the query methods we can filter by that.

[1]: https://en.wikipedia.org/wiki/Finite-difference_time-domain_method
[2]: https://en.wikipedia.org/wiki/Finite_element_exterior_calculus
