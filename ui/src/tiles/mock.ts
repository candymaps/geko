export const mock = {
  pipeline: [
    { stage: "Download", state: "done", note: "OSM extracts, cached", took: "2 min" },
    { stage: "Cut to scope", state: "done", note: "water and land polygons", took: "1 min" },
    { stage: "Planetiler", state: "done", note: "4 GB heap, 8 threads", took: "10 min" },
    { stage: "Index", state: "done", note: "pinned in RAM", took: "1 min" },
  ],
};
