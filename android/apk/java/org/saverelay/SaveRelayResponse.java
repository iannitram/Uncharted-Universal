// Ported to Java from iannitram/save-relay (MIT), bindings/android/src/main/kotlin/org/saverelay/SaveRelayHost.kt.
package org.saverelay;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;

public final class SaveRelayResponse {
    public final int status;
    public final String headers;
    public final byte[] body;

    public SaveRelayResponse(int status, String headers, byte[] body) {
        this.status = status;
        this.headers = headers;
        this.body = body == null ? new byte[0] : body;
    }

    public byte[] toWire() {
        byte[] headerBytes = (headers == null ? "" : headers).getBytes(StandardCharsets.UTF_8);
        ByteBuffer wire = ByteBuffer.allocate(8 + headerBytes.length + body.length).order(ByteOrder.BIG_ENDIAN);
        wire.putInt(status);
        wire.putInt(headerBytes.length);
        wire.put(headerBytes);
        wire.put(body);
        return wire.array();
    }
}
