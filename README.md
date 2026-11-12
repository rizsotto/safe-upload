# Safe Upload

Safe Upload cleans images before you upload them. It removes location, camera
and other metadata. It shrinks the file to a size you pick, like "under 5 MB".
You can paint over parts of the image. The output gets a neutral file name. The
original file is never changed.

It will come as a command line tool, a GNOME app, and a file chooser portal
backend, so the cleaning can happen right when an app asks you to pick a file.

## Status

Pre-alpha. Spec stage. There is no working code yet.

## Develop

All work happens in a devcontainer (Fedora, Rust, GTK4, libadwaita).

- VS Code: install the Dev Containers extension, open this folder, then pick
  "Reopen in Container".
- Command line with podman:

  ```sh
  devcontainer up --workspace-folder . --docker-path podman
  devcontainer exec --workspace-folder . --docker-path podman bash
  ```

## Docs

- [spec/SPEC.md](spec/SPEC.md): what we build and the rules.
- [spec/workflow.md](spec/workflow.md): how we work, spec first.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
