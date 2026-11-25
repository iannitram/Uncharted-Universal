package org.uncharted.universal;

import android.content.Context;

import org.json.JSONObject;
import org.saverelay.SaveRelay;
import org.saverelay.SaveRelayHost;
import org.saverelay.SaveRelayResponse;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.security.MessageDigest;
import java.util.List;
import java.util.Map;

final class Relay {
    static final int OFF = 0;
    static final int ON = 1;
    static final int SIGN_IN_AGAIN = 2;
    static final int BROKEN = 3;

    static final String DOWNLOAD_ACCOUNT = "download:dropbox";
    static volatile String lastHttpError;

    private static SaveRelay relay;

    private Relay() {}

    private static String files(Context c) {
        return c.getApplicationContext().getFilesDir().getAbsolutePath();
    }

    private static File credFile(String filesRoot, String account) {
        try {
            MessageDigest md = MessageDigest.getInstance("SHA-256");
            byte[] digest = md.digest(account.getBytes(StandardCharsets.UTF_8));
            StringBuilder hex = new StringBuilder();
            for (byte b : digest) {
                hex.append(String.format("%02x", b));
            }
            File dir = new File(filesRoot, "relay-creds");
            if (!dir.exists()) {
                //noinspection ResultOfMethodCallIgnored
                dir.mkdirs();
            }
            return new File(dir, hex.toString() + ".cred");
        } catch (Exception e) {
            return new File(filesRoot, "relay-creds/default.cred");
        }
    }

    static byte[] credGet(String filesRoot, String account) {
        if (account == null || account.isEmpty()) return null;
        try {
            File f = credFile(filesRoot, account);
            if (!f.isFile()) return null;
            return Files.readAllBytes(f.toPath());
        } catch (Throwable e) {
            return null;
        }
    }

    static boolean credSet(String filesRoot, String account, String secret) {
        if (account == null || account.isEmpty() || secret == null) return false;
        try {
            File f = credFile(filesRoot, account);
            try (FileOutputStream out = new FileOutputStream(f)) {
                out.write(secret.getBytes(StandardCharsets.UTF_8));
            }
            //noinspection ResultOfMethodCallIgnored
            f.setReadable(false, false);
            //noinspection ResultOfMethodCallIgnored
            f.setReadable(true, true);
            //noinspection ResultOfMethodCallIgnored
            f.setWritable(false, false);
            //noinspection ResultOfMethodCallIgnored
            f.setWritable(true, true);
            return true;
        } catch (Throwable e) {
            return false;
        }
    }

    static boolean credDelete(String filesRoot, String account) {
        if (account == null || account.isEmpty()) return true;
        File f = credFile(filesRoot, account);
        return !f.exists() || f.delete();
    }

    static synchronized SaveRelay saveRelay(Context c) {
        if (relay == null) {
            String f = files(c);
            new File(f, "relay-data").mkdirs();
            new File(f, "relay-config").mkdirs();
            relay = new SaveRelay(f, f + "/relay-data", f + "/relay-config", null, new Host(f));
        }
        return relay;
    }

    private static File configFile(Context c) {
        return new File(files(c), "relay-config/save-relay/config.json");
    }

    private static String account(Context c) {
        try {
            File f = configFile(c);
            if (!f.isFile()) return null;
            JSONObject config = new JSONObject(new String(Files.readAllBytes(f.toPath()), StandardCharsets.UTF_8));
            if (!"dropbox".equals(config.optString("server"))) return null;
            String user = config.optString("user");
            return user.isEmpty() ? null : "dropbox:" + user;
        } catch (Exception e) {
            return null;
        }
    }

    static int state(Context c) {
        File f = configFile(c);
        if (!f.isFile()) return OFF;
        try {
            JSONObject config = new JSONObject(new String(Files.readAllBytes(f.toPath()), StandardCharsets.UTF_8));
            String server = config.optString("server");
            if (server.isEmpty()) return OFF;
            if (!"dropbox".equals(server)) return BROKEN;
            String user = config.optString("user");
            if (user.isEmpty()) return SIGN_IN_AGAIN;
            byte[] tok = credGet(files(c), "dropbox:" + user);
            return (tok != null && tok.length > 0) ? ON : SIGN_IN_AGAIN;
        } catch (Exception e) {
            return BROKEN;
        }
    }

    static String dropboxRefresh(Context c) {
        String f = files(c);
        String acct = account(c);
        if (acct != null) {
            byte[] secret = credGet(f, acct);
            if (secret != null && secret.length > 0) {
                return new String(secret, StandardCharsets.UTF_8);
            }
        }
        byte[] dlSecret = credGet(f, DOWNLOAD_ACCOUNT);
        return (dlSecret != null && dlSecret.length > 0)
                ? new String(dlSecret, StandardCharsets.UTF_8)
                : null;
    }

    static void signedIn(Context c) {
        String acct = account(c);
        if (acct == null) return;
        byte[] tok = credGet(files(c), acct);
        if (tok != null && tok.length > 0) {
            credSet(files(c), DOWNLOAD_ACCOUNT, new String(tok, StandardCharsets.UTF_8));
        }
    }

    static void dropDropboxToken(Context c) {
        String f = files(c);
        String acct = account(c);
        if (acct != null) credDelete(f, acct);
        credDelete(f, DOWNLOAD_ACCOUNT);
    }

    static boolean turnOff(Context c) {
        String f = files(c);
        String acct = account(c);
        if (acct != null) {
            byte[] tok = credGet(f, acct);
            if (tok != null && tok.length > 0) {
                credSet(f, DOWNLOAD_ACCOUNT, new String(tok, StandardCharsets.UTF_8));
            }
            credDelete(f, acct);
        }
        File cfg = configFile(c);
        return !cfg.exists() || cfg.delete();
    }

    static boolean signOut(Context c) {
        dropDropboxToken(c);
        File cfg = configFile(c);
        return !cfg.exists() || cfg.delete();
    }

    static JSONObject pullSaves(Context c, GameInfo game, String resolve) {
        try {
            String raw = saveRelay(c).pull(game.manifestJson, game.savesDir(c).getAbsolutePath(), resolve);
            return new JSONObject(raw);
        } catch (Exception e) {
            return errorEnvelope(e);
        }
    }

    static JSONObject pushSaves(Context c, GameInfo game, String resolve) {
        try {
            String raw = saveRelay(c).push(game.manifestJson, game.savesDir(c).getAbsolutePath(), resolve);
            return new JSONObject(raw);
        } catch (Exception e) {
            return errorEnvelope(e);
        }
    }

    private static JSONObject errorEnvelope(Exception e) {
        JSONObject out = new JSONObject();
        try {
            out.put("ok", false);
            JSONObject err = new JSONObject();
            err.put("message", e.getMessage() == null ? e.toString() : e.getMessage());
            out.put("error", err);
        } catch (Exception ignored) {
        }
        return out;
    }

    private static final class Host implements SaveRelayHost {
        private final String files;

        Host(String files) {
            this.files = files;
        }

        @Override
        public byte[] credentialGet(String account) {
            return credGet(files, account);
        }

        @Override
        public boolean credentialSet(String account, String secret) {
            return credSet(files, account, secret);
        }

        @Override
        public boolean credentialDelete(String account) {
            return credDelete(files, account);
        }

        @Override
        public byte[] http(String method, String url, String headers, byte[] body, long maxResponseBytes) {
            HttpURLConnection c = null;
            try {
                c = (HttpURLConnection) new URL(url).openConnection();
                c.setConnectTimeout(20000);
                c.setReadTimeout(60000);
                c.setInstanceFollowRedirects(false);
                c.setRequestMethod(method);
                boolean contentType = false, encoding = false;
                if (headers != null) {
                    for (String line : headers.split("\n")) {
                        int colon = line.indexOf(':');
                        if (colon <= 0) continue;
                        String name = line.substring(0, colon).trim();
                        String value = line.substring(colon + 1).trim();
                        if (name.equalsIgnoreCase("Content-Length") || name.equalsIgnoreCase("Host")) continue;
                        if (name.equalsIgnoreCase("Content-Type")) contentType = true;
                        if (name.equalsIgnoreCase("Accept-Encoding")) encoding = true;
                        c.setRequestProperty(name, value);
                    }
                }
                if (method.equalsIgnoreCase("POST") && !contentType) {
                    c.setRequestProperty("Content-Type", "application/octet-stream");
                }
                if (!encoding) {
                    c.setRequestProperty("Accept-Encoding", "identity");
                }
                if (body != null && body.length > 0) {
                    c.setDoOutput(true);
                    c.setFixedLengthStreamingMode(body.length);
                    try (OutputStream o = c.getOutputStream()) {
                        o.write(body);
                    }
                }
                int status = c.getResponseCode();
                StringBuilder lines = new StringBuilder();
                for (Map.Entry<String, List<String>> h : c.getHeaderFields().entrySet()) {
                    if (h.getKey() == null) continue;
                    for (String v : h.getValue()) {
                        lines.append(h.getKey()).append(": ").append(v).append('\n');
                    }
                }
                InputStream in = status < 400 ? c.getInputStream() : c.getErrorStream();
                ByteArrayOutputStream out = new ByteArrayOutputStream();
                if (in != null) {
                    byte[] buf = new byte[16384];
                    int n;
                    while ((n = in.read(buf)) > 0) {
                        out.write(buf, 0, n);
                        if (maxResponseBytes > 0 && out.size() > maxResponseBytes) {
                            lastHttpError = "the answer is over " + maxResponseBytes + " bytes (" + method + " "
                                    + url.replaceAll("\\?.*", "") + ")";
                            return null;
                        }
                    }
                    in.close();
                }
                return new SaveRelayResponse(status, lines.toString(), out.toByteArray()).toWire();
            } catch (Throwable e) {
                String where = method + " " + url.replaceAll("\\?.*", "");
                lastHttpError = e.getClass().getSimpleName()
                        + (e.getMessage() == null ? "" : ": " + e.getMessage())
                        + " (" + where + ")";
                android.util.Log.w("UnchartedAPK", "relay http " + where + " failed", e);
                return null;
            } finally {
                if (c != null) c.disconnect();
            }
        }
    }
}
