import {DirectAccessPage as DirectAccessPageModel} from "../types/profile.ts";
import {CSSProperties} from "preact";
import DirectAccessStationKey from "./ui/DirectAccessStationKey.tsx";
import {clsx} from "clsx";
import ButtonLabel from "./ui/ButtonLabel.tsx";
import Button from "./ui/Button.tsx";
import {useProfileStore} from "../stores/profile-store.ts";
import {useCallState} from "../hooks/call-state-hook.ts";
import ClientPage from "./ClientPage.tsx";
import {CustomButtonColor} from "../types/custom-button-colors.ts";

type DirectAccessPageProps = {
    data: DirectAccessPageModel;
};

function DirectAccessPage({data}: DirectAccessPageProps) {
    const style: CSSProperties = {
        gridTemplateRows: `repeat(${data.rows}, 1fr)`,
    };

    return (
        <div className="w-full h-full overflow-auto">
            <div className="w-min min-h-full py-3 px-2 grid grid-flow-col gap-2" style={style}>
                {data.keys !== undefined ? (
                    data.keys.map((key, index) =>
                        key.page !== undefined ? (
                            <DirectAccessSubpageKey
                                key={index}
                                label={key.label}
                                page={key.page}
                                color={key.color}
                                parent={data}
                            />
                        ) : (
                            <DirectAccessStationKey
                                key={index}
                                data={key}
                                className={
                                    data.rows !== undefined && data.rows > 6
                                        ? "leading-4!"
                                        : "leading-4.5!"
                                }
                            />
                        ),
                    )
                ) : data.clientPage !== undefined ? (
                    <ClientPage config={data.clientPage} />
                ) : (
                    <></>
                )}
            </div>
        </div>
    );
}

type DirectAccessSubpageKeyProps = {
    label: string[];
    page: DirectAccessPageModel;
    parent: DirectAccessPageModel;
    color: CustomButtonColor | undefined;
    className?: string;
};

function DirectAccessSubpageKey(props: DirectAccessSubpageKeyProps) {
    const {beingCalled, isRejected, color} = useCallState(props.page, props.color);
    const setSubpage = useProfileStore(state => state.setSubpage);

    return (
        <Button
            color={color}
            highlight={beingCalled || isRejected ? "green" : undefined}
            className={clsx(
                props.className,
                "w-25 h-full rounded",
                color === "gray" ? "p-1.5" : "p-[calc(0.375rem+1px)]",
            )}
            onClick={() => setSubpage(props.page, props.parent)}
        >
            <ButtonLabel label={props.label} />
        </Button>
    );
}

export default DirectAccessPage;
