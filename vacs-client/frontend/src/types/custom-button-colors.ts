export type CustomButtonColor =
    | "clay"
    | "blush"
    | "lilac"
    | "mint"
    | "lavender"
    | "taupe"
    | "cadet"
    | "steel"
    | "umber"
    | "lagoon";

export const CustomButtonColors: Record<CustomButtonColor, string> = {
    clay: "bg-[#e68765] border-t-[#eb9f84] border-l-[#eb9f84] border-r-[#b86c51] border-b-[#b86c51]",
    blush: "bg-[#ebc3bc] border-t-[#f0d2cd] border-l-[#f0d2cd] border-r-[#b0928d] border-b-[#b0928d]",
    lilac: "bg-[#db9acc] border-t-[#e4b3d9] border-l-[#e4b3d9] border-r-[#a47499] border-b-[#a47499]",
    mint: "bg-[#abdecc] border-t-[#c0e6d9] border-l-[#c0e6d9] border-r-[#80a799] border-b-[#80a799]",
    lavender:
        "bg-[#b9abde] border-t-[#cbc0e6] border-l-[#cbc0e6] border-r-[#8b80a7] border-b-[#8b80a7]",
    taupe: "bg-[#bba58f] border-t-[#ccbcab] border-l-[#ccbcab] border-r-[#8c7c6b] border-b-[#8c7c6b]",
    cadet: "bg-[#8ca1d1] border-t-[#a9b9dd] border-l-[#a9b9dd] border-r-[#69799d] border-b-[#69799d]",
    steel: "bg-[#8fa6b4] border-t-[#abbcc7] border-l-[#abbcc7] border-r-[#6b7d87] border-b-[#6b7d87]",
    umber: "bg-[#a98874] border-t-[#bca391] border-l-[#bca391] border-r-[#7e6655] border-b-[#7e6655]",
    lagoon: "bg-[#73b7c2] border-t-[#95cad1] border-l-[#95cad1] border-r-[#598b92] border-b-[#598b92]",
};

export const CustomActiveButtonColors: Record<CustomButtonColor, string> = {
    clay: "active:border-r-[#eb9f84] active:border-b-[#eb9f84] active:border-t-[#b86c51] active:border-l-[#b86c51]",
    blush: "active:border-r-[#f0d2cd] active:border-b-[#f0d2cd] active:border-t-[#b0928d] active:border-l-[#b0928d]",
    lilac: "active:border-r-[#e4b3d9] active:border-b-[#e4b3d9] active:border-t-[#a47499] active:border-l-[#a47499]",
    mint: "active:border-r-[#c0e6d9] active:border-b-[#c0e6d9] active:border-t-[#80a799] active:border-l-[#80a799]",
    lavender:
        "active:border-r-[#cbc0e6] active:border-b-[#cbc0e6] active:border-t-[#8b80a7] active:border-l-[#8b80a7]",
    taupe: "active:border-r-[#ccbcab] active:border-b-[#ccbcab] active:border-t-[#8c7c6b] active:border-l-[#8c7c6b]",
    cadet: "active:border-r-[#a9b9dd] active:border-b-[#a9b9dd] active:border-t-[#69799d] active:border-l-[#69799d]",
    steel: "active:border-r-[#abbcc7] active:border-b-[#abbcc7] active:border-t-[#6b7d87] active:border-l-[#6b7d87]",
    umber: "active:border-r-[#bca391] active:border-b-[#bca391] active:border-t-[#7e6655] active:border-l-[#7e6655]",
    lagoon: "active:border-r-[#95cad1] active:border-b-[#95cad1] active:border-t-[#598b92] active:border-l-[#598b92]",
};

export const CustomForceDisabledButtonColors: Record<CustomButtonColor, string> = {
    clay: "border-[#b86c51]! border!",
    blush: "border-[#b0928d]! border!",
    lilac: "border-[#a47499]! border!",
    mint: "border-[#80a799]! border!",
    lavender: "border-[#8b80a7]! border!",
    taupe: "border-[#8c7c6b]! border!",
    cadet: "border-[#69799d]! border!",
    steel: "border-[#6b7d87]! border!",
    umber: "border-[#7e6655]! border!",
    lagoon: "border-[#598b92]! border!",
};

export const CustomButtonHighlightColors: Record<CustomButtonColor, string> = {
    clay: "bg-[#e68765]",
    blush: "bg-[#ebc3bc]",
    lilac: "bg-[#db9acc]",
    mint: "bg-[#abdecc]",
    lavender: "bg-[#b9abde]",
    taupe: "bg-[#bba58f]",
    cadet: "bg-[#8ca1d1]",
    steel: "bg-[#8fa6b4]",
    umber: "bg-[#a98874]",
    lagoon: "bg-[#73b7c2]",
};
