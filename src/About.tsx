import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import rustyNotesIcon from "../src-tauri/icons/128x128.png";

interface BuildInfo {
  version: string;
  operating_system: string;
  architecture: string;
  profile: string;
}

export function About({ onClose }: { onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [info, setInfo] = useState<BuildInfo | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    dialog.current?.showModal();
    void invoke<BuildInfo>("get_build_info")
      .then(setInfo)
      .catch(() => setError(true));
  }, []);

  return <dialog ref={dialog} className="settings-dialog about-dialog" aria-labelledby="about-title" onCancel={onClose}>
    <div className="about-heading">
      <img src={rustyNotesIcon} width="64" height="64" alt="" />
      <div>
        <h2 id="about-title">RustyNotes</h2>
        <p>Local-first Markdown notes with manual Nextcloud synchronization.</p>
      </div>
    </div>
    {!info && !error && <p role="status">Reading build information…</p>}
    {info && <dl className="build-info">
      <div><dt>Version</dt><dd>{info.version}</dd></div>
      <div><dt>Platform</dt><dd>{info.operating_system} · {info.architecture}</dd></div>
      <div><dt>Build</dt><dd>{info.profile}</dd></div>
    </dl>}
    {error && <p role="alert">Build information could not be read.</p>}
    <p>Copyright © 2026 Jim Kelley. Released under the MIT License.</p>
    <div className="settings-actions"><button onClick={onClose}>Close</button></div>
  </dialog>;
}
