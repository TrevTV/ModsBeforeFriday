import '../css/ModCard.css';
import { Mod } from '../Models'
import { Slider } from './Slider';
import TrashCan from '../icons/trash.svg';
import { confirm } from './Modal';

interface ModCardProps {
    mod: Mod,
    onEnabledChanged: (enabled: boolean) => void,
    onRemoved: () => void
    pendingChange?: boolean
}

export function ModCard(props: ModCardProps) {
    const setEnabled = (enabled: boolean) => {
        props.onEnabledChanged(enabled);
        props.mod.is_enabled = enabled;
    }

    return <div className="container modCard">
        <div className='modName'>
            <span className="nameSpan">
                <p className='nameText'>{props.mod.name}</p>
            </span>
            <p className='idVersionText'>{props.mod.id}{props.mod.version !== null && ` (v${props.mod.version})`}</p>
        </div>

        <p className='descriptionText'>{props.mod.repo_id !== null ? `Installed from Thunderstore: ${props.mod.repo_id}` : null}</p>

        <div className='modControls'>
            <button type="button" id="removeMod" onClick={async () => {
                if (
                    await confirm(
                        "Confirm removal",
                        <>
                            <p>Are you sure that you want to remove {props.mod.name}{props.mod.version !== null && ` v${props.mod.version}`}?</p>
                        </>
                    )
                ) {
                    props.onRemoved();
                }
            }}>
                <img src={TrashCan} alt="Remove mod icon" />
            </button>
            <Slider on={props.pendingChange !== undefined ? props.pendingChange : props.mod.is_enabled} valueChanged={value => {
                setEnabled(value);
            }}/>
        </div>

    </div>
}
