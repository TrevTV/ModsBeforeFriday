import { SourceUrl } from "..";
import { Modal } from "./Modal";

export function CreditsModal({ isVisible, onClose }: { isVisible: boolean, onClose: () => void }) {
    return <Modal isVisible={isVisible}>
        <h2>Credits</h2>
        <p>The BONELAB Lemonifier is based on <a href={SourceUrl}>ModsBeforeFriday</a>, Lauriethefish's mod installer for Beat Saber on Quest,
        which was adapted to target BONELAB with LemonLoader.</p>
        <p>Mods are distributed via <a href="https://thunderstore.io/c/bonelab/">Thunderstore</a>.</p>

        <h3>ModsBeforeFriday contributors</h3>
        <ul>
            <li><a href="https://github.com/FrozenAlex">FrozenAlex</a> created the drag 'n' drop system for MBF, and has provided me with much insight on UI design. Without him, the UI would be (even more of) a cluttered mess!</li>
            <li><a href="https://github.com/XoToM">XoToM</a>, a good friend of mine, created the animated background that you know and love. (although your CPU might hate it!)</li>
            <li><a href="https://github.com/AltyFox">Alteran</a>, a member of the BSMG support team, has provided invaluable feedback regarding usability, and has helped me to pinpoint and fix bugs.</li>
        </ul>

        <div className="confirmButtons">
            <button onClick={onClose}>OK</button>
        </div>
    </Modal>
}
