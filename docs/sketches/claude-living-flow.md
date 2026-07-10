# Living bean-graph — simulated request flow (#66 „Living architecture")

**Concept.** ProjectMind is a read-only current-state browser: there is no live
request stream to animate. The honest "living" reading is a *simulated* request
wave: BFS from the entry stereotypes (rest-controller/controller) along the
directed relation edges towards the repositories, rendered as marching-ants
edges + node pulses, looping. A second overlay maps real `commit_activity`
onto module halos ("the parts of the system that are alive *in the repo*").

**Question it answers.** "How does a request topologically travel through this
system, and which parts are actually being worked on right now?"

**Candidate library.** Cytoscape.js (already shipped) — `line-dash-offset`
animation, no new dependency. Explicitly *not* a particle engine.

**Mock.**
  (Ctrl)●━ants━▶(Svc)○──▶(Repo)○      wave 0: controllers pulse
  (Ctrl)●━ants━▶(Svc)○──▶(Repo)○      wave 1: services + edges
                                       wave 2: repositories … loop

**Honesty rule (#61).** The animation is labelled "simulated flow — topology
order, not runtime traffic" in the toolbar tooltip.
