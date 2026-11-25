package org.uncharted.universal;

import android.content.Context;
import android.content.SharedPreferences;
import android.net.Uri;

import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.net.URL;
import java.net.URLDecoder;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;

final class DropboxAuth {
    // save-relay's Dropbox app key (public PKCE client, App folder access).
    static final String APP_KEY = "p8xhxihp8s3gb33";
    static final String REDIRECT = "http://127.0.0.1:53710/";
    private static final int PORT = 53710;
    private static final String PREFS = "dropbox";

    private static ServerSocket waiting;
    private static int generation;
    private static int finished;
    private static String failed;
    private static boolean foreground;
    private static boolean cancelled;
    private static Context appContext;

    private DropboxAuth() {}

    private static SharedPreferences prefs(Context c) {
        return c.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    private static String refresh(Context c) {
        String relay = Relay.dropboxRefresh(c);
        return relay != null ? relay : prefs(c).getString("refresh", "");
    }

    static boolean signedIn(Context c) {
        return !refresh(c).isEmpty();
    }

    static void signOut(Context c) {
        prefs(c).edit().clear().apply();
        Relay.dropDropboxToken(c);
    }

    static boolean signOutAll(Context c) {
        prefs(c).edit().clear().apply();
        return Relay.signOut(c);
    }

    static synchronized void setForeground(boolean active) {
        foreground = active;
        if (active) DropboxAuth.class.notifyAll();
    }

    static synchronized String await(int gen) {
        while (finished < gen && generation == gen) {
            try {
                DropboxAuth.class.wait();
            } catch (InterruptedException e) {
                return "interrupted";
            }
        }
        if (generation != gen) return "another sign-in started";
        return failed;
    }

    static synchronized int current() {
        return generation;
    }

    static synchronized void cancel(int gen) {
        if (gen == generation) {
            cancelled = true;
            if (waiting != null) {
                try {
                    waiting.close();
                } catch (Exception ignored) {
                }
            }
            DropboxAuth.class.notifyAll();
        }
    }

    private static boolean waitForNetwork(int gen) {
        synchronized (DropboxAuth.class) {
            while (!foreground && !GameDownload.running && generation == gen && !cancelled) {
                try {
                    DropboxAuth.class.wait(500);
                } catch (InterruptedException e) {
                    return false;
                }
            }
            return generation == gen && !cancelled;
        }
    }

    private static String envelopeError(JSONObject envelope) {
        JSONObject e = envelope.optJSONObject("error");
        String m = e == null ? "" : e.optString("message");
        return m.isEmpty() ? "save-relay could not sign in" : m;
    }

    static synchronized String start(final Context ctx) throws Exception {
        if (waiting != null) {
            try {
                waiting.close();
            } catch (Exception ignored) {
            }
            waiting = null;
        }
        failed = null;
        cancelled = false;
        final int gen = ++generation;
        DropboxAuth.class.notifyAll();
        final Context app = ctx.getApplicationContext();
        appContext = app;
        JSONObject started = new JSONObject(
                Relay.saveRelay(app).signInStart("dropbox", REDIRECT, "Android", null));
        if (!started.optBoolean("ok")) {
            throw new Exception(envelopeError(started));
        }
        final String url = started.getJSONObject("data").getString("url");
        final String state = Uri.parse(url).getQueryParameter("state");
        if (state == null) {
            throw new Exception("save-relay sign-in returned no state");
        }
        final ServerSocket server = new ServerSocket(PORT, 4, InetAddress.getByName("127.0.0.1"));
        server.setSoTimeout(15 * 60 * 1000);
        waiting = server;
        new Thread(() -> {
            String error;
            try {
                error = serve(app, server, state, gen);
            } catch (Exception e) {
                error = server.isClosed()
                        ? "cancelled"
                        : (e.getMessage() == null ? e.toString() : e.getMessage());
            } finally {
                try {
                    server.close();
                } catch (Exception ignored) {
                }
                synchronized (DropboxAuth.class) {
                    if (waiting == server) waiting = null;
                }
            }
            synchronized (DropboxAuth.class) {
                if (gen == generation) {
                    failed = error;
                    if (error == null && !signedIn(app)) {
                        failed = "no sign-in came back";
                    }
                    finished = gen;
                    DropboxAuth.class.notifyAll();
                }
            }
        }, "dropbox-sign-in").start();
        return url;
    }

    private static String serve(Context app, ServerSocket server, String state, int gen) throws Exception {
        GameInfo game = GameInfo.forContext(app);
        while (true) {
            Socket s = server.accept();
            try {
                s.setSoTimeout(5000);
                InputStream in = s.getInputStream();
                String line = firstLine(in);
                for (String h = firstLine(in); !h.isEmpty(); h = firstLine(in)) {
                }
                String target = "", query = "";
                int sp = line.lastIndexOf(' ');
                if (line.startsWith("GET ") && sp > 4) target = line.substring(4, sp);
                int q = target.indexOf('?');
                if (q >= 0) query = target.substring(q + 1);
                String code = param(query, "code");
                String err = param(query, "error");
                String got = param(query, "state");
                if ((code == null && err == null) || !state.equals(got)) {
                    reply(s.getOutputStream(), 404,
                            page(game.shortLabel, "Not this sign-in. Go back to " + game.shortLabel + "."));
                    continue;
                }
                if (err != null) {
                    reply(s.getOutputStream(), 200,
                            page(game.shortLabel, "Dropbox sign-in was cancelled. Go back to " + game.shortLabel + "."));
                    String why = param(query, "error_description");
                    return why != null ? why : err;
                }
                reply(s.getOutputStream(), 200, back(app, game));
                try {
                    s.close();
                } catch (Exception ignored) {
                }
                if (!waitForNetwork(gen)) return "cancelled";
                String redirectUrl = "http://127.0.0.1:" + PORT + target;
                String error = null;
                for (int attempt = 0; attempt < 4; attempt++) {
                    error = null;
                    Relay.lastHttpError = null;
                    try {
                        JSONObject done = new JSONObject(Relay.saveRelay(app).signInFinish(redirectUrl));
                        if (!done.optBoolean("ok")) {
                            error = envelopeError(done);
                        } else {
                            prefs(app).edit().clear().apply();
                            Relay.signedIn(app);
                        }
                    } catch (Exception e) {
                        error = e.getMessage() == null ? e.toString() : e.getMessage();
                    }
                    if (error == null || Relay.lastHttpError == null || attempt == 3) break;
                    try {
                        Thread.sleep(300);
                    } catch (InterruptedException ignored) {
                        break;
                    }
                }
                if (error != null && Relay.lastHttpError != null) {
                    error += " [" + Relay.lastHttpError + "]";
                }
                return error;
            } catch (java.net.SocketTimeoutException ignored) {
            } catch (java.net.SocketException e) {
                if (server.isClosed()) throw e;
            } finally {
                try {
                    s.close();
                } catch (Exception ignored) {
                }
            }
        }
    }

    private static String firstLine(InputStream in) throws Exception {
        ByteArrayOutputStream b = new ByteArrayOutputStream();
        int c;
        while ((c = in.read()) >= 0 && c != '\n' && b.size() < 8192) {
            b.write(c);
        }
        return new String(b.toByteArray(), StandardCharsets.UTF_8).trim();
    }

    private static String param(String query, String name) throws Exception {
        for (String kv : query.split("&")) {
            int eq = kv.indexOf('=');
            if (eq > 0 && kv.substring(0, eq).equals(name)) {
                return URLDecoder.decode(kv.substring(eq + 1), "UTF-8");
            }
        }
        return null;
    }

    private static String back(Context app, GameInfo game) {
        String intentUri = "intent://signed-in#Intent;scheme=" + game.scheme
                + ";package=" + app.getPackageName() + ";end";
        return "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'>"
                + "<body style='font-family:sans-serif;padding:2em;background:#18202c;color:#f5f0e0;text-align:center'>"
                + "<h2>Returning to " + game.title + "</h2>"
                + "<p>Go back to the app to finish signing in.</p>"
                + "<p><a href='" + intentUri + "' style='display:inline-block;padding:16px 28px;border-radius:26px;"
                + "background:#36985c;color:white;font-size:18px;text-decoration:none'>Back to "
                + game.shortLabel + "</a></p>"
                + "<script>location.replace('" + intentUri + "')</script>";
    }

    private static String page(String title, String text) {
        return "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'>"
                + "<body style='font-family:sans-serif;padding:2em;background:#18202c;color:#f5f0e0'><h2>"
                + title + "</h2><p>"
                + text.replace("&", "&amp;").replace("<", "&lt;") + "</p>";
    }

    private static void reply(OutputStream out, int status, String body) throws Exception {
        byte[] b = body.getBytes(StandardCharsets.UTF_8);
        out.write(("HTTP/1.1 " + status + " OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: "
                + b.length + "\r\nConnection: close\r\n\r\n").getBytes(StandardCharsets.US_ASCII));
        out.write(b);
        out.flush();
    }

    private static String enc(String s) {
        try {
            return URLEncoder.encode(s, "UTF-8");
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
    }

    private static JSONObject token(String form) throws Exception {
        HttpURLConnection c = (HttpURLConnection) new URL("https://api.dropboxapi.com/oauth2/token").openConnection();
        c.setConnectTimeout(20000);
        c.setReadTimeout(30000);
        c.setRequestMethod("POST");
        c.setDoOutput(true);
        c.setRequestProperty("Content-Type", "application/x-www-form-urlencoded");
        try (OutputStream o = c.getOutputStream()) {
            o.write(form.getBytes(StandardCharsets.UTF_8));
        }
        int code = c.getResponseCode();
        InputStream in = code < 400 ? c.getInputStream() : c.getErrorStream();
        String body = in == null ? "" : new String(readAll(in), StandardCharsets.UTF_8);
        c.disconnect();
        if (code == 400 || code == 401) {
            if (body.contains("invalid_grant") && form.startsWith("grant_type=refresh_token")) {
                signOut(appContext);
                throw new GameDownload.Fatal("Dropbox no longer accepts this device's sign-in. Sign in again.");
            }
            throw new GameDownload.Fatal("Dropbox refused the sign-in (" + code + (body.isEmpty() ? "" : ": " + body) + ")");
        }
        if (code != 200) throw new java.io.IOException("Dropbox answered " + code);
        return new JSONObject(body);
    }

    static byte[] readAll(InputStream in) throws Exception {
        ByteArrayOutputStream b = new ByteArrayOutputStream();
        byte[] buf = new byte[16384];
        int n;
        while ((n = in.read(buf)) > 0) b.write(buf, 0, n);
        in.close();
        return b.toByteArray();
    }

    static synchronized String access(Context c, boolean force) throws Exception {
        appContext = c.getApplicationContext();
        SharedPreferences p = prefs(c);
        String refresh = refresh(c);
        if (refresh.isEmpty()) {
            throw new GameDownload.Fatal("Not signed in to Dropbox. Tap Download to sign in.");
        }
        String access = p.getString("access", "");
        if (!force && !access.isEmpty() && p.getLong("expires", 0) - System.currentTimeMillis() > 5 * 60 * 1000) {
            return access;
        }
        JSONObject t = token("grant_type=refresh_token&refresh_token=" + enc(refresh) + "&client_id=" + APP_KEY);
        access = t.getString("access_token");
        p.edit().putString("access", access)
                .putLong("expires", System.currentTimeMillis() + t.optLong("expires_in", 14400) * 1000).apply();
        return access;
    }
}
