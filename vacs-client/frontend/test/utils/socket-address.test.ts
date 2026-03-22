import {describe, expect, it} from "vitest";
import {parseSocketAddress} from "../../src/utils/socket-address.ts";

const DEFAULT_IP = "0.0.0.0";
const DEFAULT_PORT = 9600;

function parse(input: string): string | null {
    return parseSocketAddress(input, DEFAULT_IP, DEFAULT_PORT);
}

describe("parseSocketAddress", () => {
    describe("empty / whitespace input", () => {
        it("returns default address for empty string", () => {
            expect(parse("")).toBe("0.0.0.0:9600");
        });

        it("returns default address for whitespace-only string", () => {
            expect(parse("   ")).toBe("0.0.0.0:9600");
        });
    });

    describe("bare IPv4 (no port)", () => {
        it("appends default port", () => {
            expect(parse("1.2.3.4")).toBe("1.2.3.4:9600");
        });

        it("accepts 0.0.0.0", () => {
            expect(parse("0.0.0.0")).toBe("0.0.0.0:9600");
        });

        it("accepts 127.0.0.1", () => {
            expect(parse("127.0.0.1")).toBe("127.0.0.1:9600");
        });

        it("accepts 255.255.255.255", () => {
            expect(parse("255.255.255.255")).toBe("255.255.255.255:9600");
        });

        it("trims surrounding whitespace", () => {
            expect(parse("  10.0.0.1  ")).toBe("10.0.0.1:9600");
        });
    });

    describe("IPv4 with port", () => {
        it("keeps address as-is", () => {
            expect(parse("1.2.3.4:80")).toBe("1.2.3.4:80");
        });

        it("accepts port 1", () => {
            expect(parse("1.2.3.4:1")).toBe("1.2.3.4:1");
        });

        it("accepts port 65535", () => {
            expect(parse("1.2.3.4:65535")).toBe("1.2.3.4:65535");
        });
    });

    describe("bare IPv6 (no brackets, no port)", () => {
        it("wraps in brackets and appends default port", () => {
            expect(parse("::1")).toBe("[::1]:9600");
        });

        it("handles full IPv6 address", () => {
            expect(parse("2001:db8:85a3:0:0:8a2e:370:7334")).toBe(
                "[2001:db8:85a3:0:0:8a2e:370:7334]:9600",
            );
        });

        it("handles :: (all zeros)", () => {
            expect(parse("::")).toBe("[::]:9600");
        });

        it("handles leading ::", () => {
            expect(parse("::ffff:1")).toBe("[::ffff:1]:9600");
        });

        it("handles trailing ::", () => {
            expect(parse("fe80::")).toBe("[fe80::]:9600");
        });
    });

    describe("bracketed IPv6 without port", () => {
        it("appends default port", () => {
            expect(parse("[::1]")).toBe("[::1]:9600");
        });

        it("handles full address", () => {
            expect(parse("[2001:db8::1]")).toBe("[2001:db8::1]:9600");
        });
    });

    describe("bracketed IPv6 with port", () => {
        it("keeps address as-is", () => {
            expect(parse("[::1]:80")).toBe("[::1]:80");
        });

        it("accepts port 65535", () => {
            expect(parse("[::1]:65535")).toBe("[::1]:65535");
        });
    });

    describe("invalid inputs", () => {
        it("rejects plain text", () => {
            expect(parse("hello")).toBeNull();
        });

        it("rejects hostname", () => {
            expect(parse("localhost")).toBeNull();
        });

        it("rejects hostname with port", () => {
            expect(parse("localhost:9600")).toBeNull();
        });

        it("rejects IPv4 octet > 255", () => {
            expect(parse("256.0.0.1")).toBeNull();
        });

        it("rejects IPv4 with negative octet", () => {
            expect(parse("-1.0.0.1")).toBeNull();
        });

        it("rejects IPv4 with leading zeros", () => {
            expect(parse("01.02.03.04")).toBeNull();
        });

        it("rejects IPv4 with too few octets", () => {
            expect(parse("1.2.3")).toBeNull();
        });

        it("rejects IPv4 with too many octets", () => {
            expect(parse("1.2.3.4.5")).toBeNull();
        });

        it("rejects port 0", () => {
            expect(parse("1.2.3.4:0")).toBeNull();
        });

        it("rejects port > 65535", () => {
            expect(parse("1.2.3.4:65536")).toBeNull();
        });

        it("rejects non-numeric port", () => {
            expect(parse("1.2.3.4:abc")).toBeNull();
        });

        it("rejects port with leading zeros", () => {
            expect(parse("1.2.3.4:080")).toBeNull();
        });

        it("rejects fractional port", () => {
            expect(parse("1.2.3.4:80.5")).toBeNull();
        });

        it("rejects unclosed bracket", () => {
            expect(parse("[::1")).toBeNull();
        });

        it("rejects bracket without colon before port", () => {
            expect(parse("[::1]80")).toBeNull();
        });

        it("rejects invalid IPv6 in brackets", () => {
            expect(parse("[not:valid]")).toBeNull();
        });

        it("rejects IPv6 with multiple ::", () => {
            expect(parse("::1::2")).toBeNull();
        });

        it("rejects IPv6 hex group > 4 chars", () => {
            expect(parse("12345::1")).toBeNull();
        });

        it("rejects empty port after colon for IPv4", () => {
            expect(parse("1.2.3.4:")).toBeNull();
        });

        it("rejects empty port after bracket for IPv6", () => {
            expect(parse("[::1]:")).toBeNull();
        });
    });

    describe("custom defaults", () => {
        it("uses provided default IP and port", () => {
            expect(parseSocketAddress("", "127.0.0.1", 8080)).toBe("127.0.0.1:8080");
        });

        it("uses provided default port for bare IPv4", () => {
            expect(parseSocketAddress("10.0.0.1", "0.0.0.0", 3000)).toBe("10.0.0.1:3000");
        });

        it("uses provided default port for bare IPv6", () => {
            expect(parseSocketAddress("::1", "0.0.0.0", 443)).toBe("[::1]:443");
        });
    });
});
