// Ported to Java from iannitram/save-relay (MIT), bindings/android/src/main/kotlin/org/saverelay/SaveRelayHost.kt.
package org.saverelay;

public interface SaveRelayHost {
    byte[] credentialGet(String account);

    boolean credentialSet(String account, String secret);

    boolean credentialDelete(String account);

    // The answer as SaveRelayResponse.toWire(), or null for a connection, DNS, TLS or timeout failure.
    byte[] http(String method, String url, String headers, byte[] body, long maxResponseBytes);
}
