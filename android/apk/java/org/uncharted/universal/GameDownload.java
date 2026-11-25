package org.uncharted.universal;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.app.job.JobInfo;
import android.app.job.JobScheduler;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;
import android.os.PowerManager;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;

public class GameDownload extends Service {
    static final String EXTRA_AUTH_ONLY = "auth_only";
    static final String CHANNEL = "download";
    static final int NOTE = 1;
    static final int DONE_NOTE = 2;
    private static final int JOB_ID = 704;

    static volatile boolean running;
    static volatile long done;
    static volatile long total;
    static volatile String status = "";
    static volatile String error;
    static volatile boolean finished;

    interface ProgressSink {
        void onProgress(String text, long done, long total);
        void onFinish(String fail);
    }

    static final class Fatal extends Exception {
        Fatal(String m) {
            super(m);
        }
    }

    private static Thread worker;
    private static volatile PowerManager.WakeLock sharedWake;
    private static volatile ProgressSink activeSink;

    static boolean downloaded(Context c) {
        return GameInfo.forContext(c).isInstalled(c);
    }

    static void ensureChannel(Context c) {
        NotificationManager nm = c.getSystemService(NotificationManager.class);
        if (nm != null) {
            nm.createNotificationChannel(
                    new NotificationChannel(CHANNEL, "Game download", NotificationManager.IMPORTANCE_LOW));
        }
    }

    static void start(Context c) {
        c.startForegroundService(new Intent(c, GameDownload.class));
    }

    static void startAuthKeepAlive(Context c) {
        c.startForegroundService(new Intent(c, GameDownload.class).putExtra(EXTRA_AUTH_ONLY, true));
    }

    static void startWorker(Context c, ProgressSink sink) {
        final Context appCtx = c.getApplicationContext();
        synchronized (GameDownload.class) {
            activeSink = sink;
            if (worker == null || !worker.isAlive()) {
                running = true;
                finished = false;
                error = null;
                if (sharedWake == null) {
                    PowerManager pm = appCtx.getSystemService(PowerManager.class);
                    if (pm != null) {
                        sharedWake = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "uncharted:download");
                    }
                }
                if (sharedWake != null && !sharedWake.isHeld()) {
                    sharedWake.acquire();
                }
                worker = new Thread(() -> runDownload(appCtx), "game-download");
                worker.start();
            }
        }
    }

    static void stopWorker() {
        synchronized (GameDownload.class) {
            if (worker != null) worker.interrupt();
        }
        running = false;
        releaseWake();
    }

    private static void releaseWake() {
        try {
            if (sharedWake != null && sharedWake.isHeld()) sharedWake.release();
        } catch (Exception ignored) {
        }
    }

    static Notification buildNote(Context c, String text, long cur, long max) {
        GameInfo game = GameInfo.forContext(c);
        Intent open = new Intent(c, Home.class);
        PendingIntent pi = PendingIntent.getActivity(c, 0, open, PendingIntent.FLAG_IMMUTABLE);
        Notification.Builder b = new Notification.Builder(c, CHANNEL)
                .setSmallIcon(android.R.drawable.stat_sys_download)
                .setContentTitle(game.shortLabel)
                .setContentText(text)
                .setContentIntent(pi)
                .setOngoing(true);
        if (max > 0) {
            b.setProgress(1000, (int) Math.min(1000L, cur * 1000L / max), false);
        } else {
            b.setProgress(0, 0, true);
        }
        return b.build();
    }

    static void postDoneNotification(Context c, String fail) {
        NotificationManager nm = c.getSystemService(NotificationManager.class);
        if (nm == null) return;
        GameInfo game = GameInfo.forContext(c);
        Intent open = new Intent(c, Home.class);
        PendingIntent pi = PendingIntent.getActivity(c, 0, open, PendingIntent.FLAG_IMMUTABLE);
        Notification note = new Notification.Builder(c, CHANNEL)
                .setSmallIcon(fail == null ? android.R.drawable.stat_sys_download_done : android.R.drawable.stat_notify_error)
                .setContentTitle(game.shortLabel)
                .setContentText(fail == null ? "Game files ready to play" : fail)
                .setContentIntent(pi)
                .setAutoCancel(true)
                .build();
        nm.notify(DONE_NOTE, note);
    }

    private static void updateStatus(String msg, long cur, long max) {
        status = msg;
        done = cur;
        total = max;
        ProgressSink sink = activeSink;
        if (sink != null) sink.onProgress(msg, cur, max);
    }

    private static void runDownload(Context ctx) {
        String fail = null;
        try {
            if (!DropboxAuth.signedIn(ctx)) {
                updateStatus("Waiting for Dropbox sign-in in browser...", 0, 0);
                String authErr = DropboxAuth.await(DropboxAuth.current());
                if (authErr != null) {
                    throw new Fatal("Dropbox sign-in: " + authErr);
                }
            }
            GameInfo game = GameInfo.forContext(ctx);
            updateStatus("Fetching manifest from Dropbox...", 0, 0);
            byte[] manifestBytes = fetchDropbox(ctx, game.remoteDir + "/manifest.json", null, 0);
            JSONObject manifest = new JSONObject(new String(manifestBytes, StandardCharsets.UTF_8));
            JSONArray files = manifest.getJSONArray("files");

            long sum = 0;
            for (int i = 0; i < files.length(); i++) {
                sum += files.getJSONObject(i).optLong("size", 0);
            }
            File partial = game.partialDir(ctx);
            File dest = game.gameDir(ctx);
            partial.mkdirs();
            dest.mkdirs();

            long completed = 0;
            for (int i = 0; i < files.length(); i++) {
                JSONObject entry = files.getJSONObject(i);
                String rel = entry.getString("path");
                long size = entry.getLong("size");
                String sha256 = entry.optString("sha256", "");

                File target = new File(partial, rel);
                File parent = target.getParentFile();
                if (parent != null) parent.mkdirs();

                long have = target.isFile() ? target.length() : 0;
                if (have < size || size == 0) {
                    updateStatus("Downloading " + rel + " (" + (i + 1) + "/" + files.length() + ")",
                            completed + have, sum);
                    fetchDropbox(ctx, game.remoteDir + "/" + rel, target, have);
                }
                if (!sha256.isEmpty() && !sha256Matches(target, sha256)) {
                    //noinspection ResultOfMethodCallIgnored
                    target.delete();
                    throw new Fatal("Checksum mismatch for " + rel + ". Tap Download to retry.");
                }
                completed += size;
                updateStatus("Verified " + rel, completed, sum);
            }

            for (int i = 0; i < files.length(); i++) {
                String rel = files.getJSONObject(i).getString("path");
                File src = new File(partial, rel);
                File out = new File(dest, rel);
                File outParent = out.getParentFile();
                if (outParent != null) outParent.mkdirs();
                if (out.exists()) {
                    //noinspection ResultOfMethodCallIgnored
                    out.delete();
                }
                if (!src.renameTo(out)) {
                    throw new IOException("Failed to move " + rel + " into place");
                }
            }
            try (FileOutputStream out = new FileOutputStream(new File(dest, "manifest.json"))) {
                out.write(manifestBytes);
            }
            try (FileOutputStream out = new FileOutputStream(new File(dest, ".installed"))) {
                out.write("ok\n".getBytes(StandardCharsets.UTF_8));
            }
            finished = true;
            updateStatus("Game files ready", sum, sum);
        } catch (Fatal f) {
            fail = f.getMessage();
        } catch (Throwable e) {
            fail = e.getMessage() == null ? e.toString() : e.getMessage();
        } finally {
            error = fail;
            running = false;
            releaseWake();
            ProgressSink sink = activeSink;
            if (sink != null) sink.onFinish(fail);
        }
    }

    private static byte[] fetchDropbox(Context ctx, String path, File into, long from) throws Exception {
        String token = DropboxAuth.access(ctx, false);
        HttpURLConnection c = (HttpURLConnection)
                new URL("https://content.dropboxapi.com/2/files/download").openConnection();
        c.setConnectTimeout(20000);
        c.setReadTimeout(60000);
        c.setRequestMethod("POST");
        c.setRequestProperty("Authorization", "Bearer " + token);
        c.setRequestProperty("Dropbox-API-Arg", new JSONObject().put("path", path).toString());
        c.setRequestProperty("Content-Type", "application/octet-stream");
        if (from > 0) {
            c.setRequestProperty("Range", "bytes=" + from + "-");
        }
        int code = c.getResponseCode();
        if (code >= 400) {
            InputStream es = c.getErrorStream();
            String body = es == null ? "" : new String(DropboxAuth.readAll(es), StandardCharsets.UTF_8);
            c.disconnect();
            if (code == 409 && body.contains("not_found")) {
                throw new Fatal("The game files are not in Dropbox yet (" + path.substring(1)
                        + " in Save Relay's Dropbox app folder).");
            }
            if (code == 401) {
                DropboxAuth.signOut(ctx);
                throw new Fatal("Dropbox refused the sign-in. Sign in again.");
            }
            throw new Fatal("Dropbox answered " + code + (body.isEmpty() ? "" : ": " + body));
        }
        if (into == null) {
            byte[] data = DropboxAuth.readAll(c.getInputStream());
            c.disconnect();
            return data;
        }
        try (InputStream in = c.getInputStream();
             FileOutputStream out = new FileOutputStream(into, from > 0 && code == 206)) {
            byte[] buf = new byte[65536];
            int n;
            while ((n = in.read(buf)) > 0) {
                if (Thread.currentThread().isInterrupted()) {
                    throw new Fatal("Download cancelled");
                }
                out.write(buf, 0, n);
                done += n;
            }
        } finally {
            c.disconnect();
        }
        return null;
    }

    private static boolean sha256Matches(File file, String expectedHex) throws Exception {
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        try (FileInputStream in = new FileInputStream(file)) {
            byte[] buf = new byte[65536];
            int n;
            while ((n = in.read(buf)) > 0) {
                md.update(buf, 0, n);
            }
        }
        byte[] digest = md.digest();
        StringBuilder hex = new StringBuilder();
        for (byte b : digest) {
            hex.append(String.format("%02x", b));
        }
        return hex.toString().equalsIgnoreCase(expectedHex);
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        ensureChannel(this);
        boolean authOnly = intent != null && intent.getBooleanExtra(EXTRA_AUTH_ONLY, false);
        String msg = authOnly
                ? "Waiting for Dropbox sign-in..."
                : (status.isEmpty() ? "Starting download" : status);
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTE, buildNote(this, msg, done, total),
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC);
        } else {
            startForeground(NOTE, buildNote(this, msg, done, total));
        }
        if (authOnly) {
            final int gen = DropboxAuth.current();
            new Thread(() -> {
                DropboxAuth.await(gen);
                stopSelf(startId);
            }, "dropbox-auth-keepalive").start();
            return START_NOT_STICKY;
        }
        startWorker(this, new ProgressSink() {
            @Override
            public void onProgress(String text, long cur, long max) {
                NotificationManager nm = getSystemService(NotificationManager.class);
                if (nm != null) nm.notify(NOTE, buildNote(GameDownload.this, text, cur, max));
            }

            @Override
            public void onFinish(String fail) {
                postDoneNotification(GameDownload.this, fail);
                stopSelf();
            }
        });
        return START_NOT_STICKY;
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
