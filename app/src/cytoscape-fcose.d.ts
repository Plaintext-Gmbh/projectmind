/// Ambient module declaration for `cytoscape-fcose`, which ships no types.
/// The package's default export is a Cytoscape extension registration
/// function passed to `cytoscape.use(...)`.
declare module 'cytoscape-fcose' {
  import type { Ext } from 'cytoscape';
  const fcose: Ext;
  export default fcose;
}
