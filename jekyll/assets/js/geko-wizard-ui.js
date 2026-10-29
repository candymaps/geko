// The modal of the wizard: reads the form, checks it and downloads the zip (geko-wizard.js writes it).
(function () {
  "use strict";
  const dialog = document.getElementById("wizard-dialog");
  const form = document.getElementById("wizard-form");
  if (!dialog || !form || !window.GekoWizard) return;

  const errors = document.getElementById("wizard-errors");
  const text = (name) => form.elements[name].value.trim();
  const checked = (name) => form.elements[name].checked;
  const whole = (name) => (/^\d+$/.test(text(name)) ? Number(text(name)) : NaN);

  function read() {
    const modules = ["geocoder", "tiles"].map((name) => ({
      name,
      enabled: checked(`${name}-enabled`),
      replicas: whole(`${name}-replicas`),
      cpu: text(`${name}-cpu`),
      memory: text(`${name}-memory`),
      buildCpu: text(`${name}-buildCpu`),
      buildMemory: text(`${name}-buildMemory`),
    }));
    const scope = ["ES", "LU"]
      .filter((country) => checked(`scope-${country}`))
      .map((country) => ({ country, regions: text(`regions-${country}`).split(/[\s,]+/).filter(Boolean).map((r) => r.toUpperCase()) }));
    return {
      namespace: text("namespace"),
      host: text("host"),
      storage: text("storage"),
      storageClass: text("storageClass"),
      accessMode: text("accessMode"),
      tag: text("tag"),
      imagePrefix: text("imagePrefix"),
      operatorImage: text("operatorImage"),
      modules,
      scope,
      acceptLicenses: checked("acceptLicenses"),
      maxAgeDays: whole("maxAgeDays"),
    };
  }

  function show(messages) {
    errors.replaceChildren(...messages.map((message) => Object.assign(document.createElement("li"), { textContent: message })));
    errors.hidden = messages.length === 0;
  }

  function download(blob) {
    const link = Object.assign(document.createElement("a"), { href: URL.createObjectURL(blob), download: "geko-k8s.zip" });
    document.body.append(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(link.href);
  }

  document.getElementById("wizard-open").addEventListener("click", () => {
    show([]);
    dialog.showModal();
  });
  document.getElementById("wizard-close").addEventListener("click", () => dialog.close());
  document.getElementById("wizard-cancel").addEventListener("click", () => dialog.close());
  dialog.addEventListener("click", (event) => {
    if (event.target === dialog) dialog.close();
  });

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const options = read();
    const problems = window.GekoWizard.validate(options);
    show(problems);
    if (problems.length) return;
    download(window.GekoWizard.zip(window.GekoWizard.bundle(options)));
    dialog.close();
  });
})();
