import {create} from "zustand/react";

type Page = "phone" | "radio" | "split";
export type Menu = "settings" | "mission" | "telephone" | "playback";

type SettingsSubmenu =
    | "settings-transmit"
    | "settings-hotkeys"
    | "settings-call"
    | "settings-advanced"
    | "settings-joystick-devices";
export type Submenu = SettingsSubmenu;

type NavigationState = {
    page: Page;
    menu: Menu | undefined;
    submenu: Submenu | undefined;
    previous: Pick<NavigationState, "page" | "menu" | "submenu">;
    setPage: (page: Page) => void;
    goToPage: (page: Page) => void;
    openMenu: (menu: Menu) => void;
    closeMenu: () => void;
    openSettingsSubmenu: (submenu: SettingsSubmenu) => void;
};

export const useNavigationStore = create<NavigationState>()(set => ({
    page: "phone",
    menu: undefined,
    submenu: undefined,
    previous: {page: "phone", menu: undefined, submenu: undefined},
    setPage: page => set(prev => ({page, previous: previous(prev)})),
    goToPage: page => {
        set(prev => ({page, menu: undefined, submenu: undefined, previous: previous(prev)}));
    },
    openMenu: menu => set(prev => ({menu, submenu: undefined, previous: previous(prev)})),
    closeMenu: () => set(prev => ({menu: undefined, submenu: undefined, previous: previous(prev)})),
    openSettingsSubmenu: (submenu: SettingsSubmenu) =>
        set(prev => ({menu: "settings", submenu, previous: previous(prev)})),
}));

const previous = (prev: NavigationState): Pick<NavigationState, "page" | "menu" | "submenu"> => ({
    page: prev.page,
    menu: prev.menu,
    submenu: prev.submenu,
});

export const setPage = (page: Page) => useNavigationStore.getState().setPage(page);

export const goToPage = (page: Page) => useNavigationStore.getState().goToPage(page);

export const openMenu = (menu: Menu) => useNavigationStore.getState().openMenu(menu);

export const closeMenu = () => useNavigationStore.getState().closeMenu();

export const openSettingsSubmenu = (submenu: SettingsSubmenu) =>
    useNavigationStore.getState().openSettingsSubmenu(submenu);
