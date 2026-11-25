package org.uncharted.universal;

import android.Manifest;
import android.app.Activity;
import android.app.AlertDialog;
import android.app.NativeActivity;
import android.content.Intent;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.content.res.Configuration;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.DisplayMetrics;
import android.view.Gravity;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;

import org.json.JSONObject;

import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;

public class Home extends Activity {
    private static final int PICK_FOLDER = 1;
    private static final int REQ_NOTIFICATIONS = 2;

    private GameInfo game;
    private LinearLayout box;
    private TextView message;
    private TextView progress;
    private ProgressBar bar;
    private boolean busy;
    private boolean askedNotifications;
    private final Handler ticks = new Handler(Looper.getMainLooper());

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        game = GameInfo.forContext(this);
        applyPixelFoldOrientation();
        show(getIntent() != null ? getIntent().getStringExtra("error") : null);
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        show(null);
    }

    @Override
    public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        applyPixelFoldOrientation();
        show(null);
    }

    @Override
    protected void onResume() {
        super.onResume();
        DropboxAuth.setForeground(true);
        ticks.removeCallbacksAndMessages(null);
        ticks.post(this::tick);
    }

    @Override
    protected void onPause() {
        super.onPause();
        DropboxAuth.setForeground(false);
        ticks.removeCallbacksAndMessages(null);
    }

    private void applyPixelFoldOrientation() {
        int sw = getResources().getConfiguration().smallestScreenWidthDp;
        setRequestedOrientation(sw >= 600
                ? ActivityInfo.SCREEN_ORIENTATION_FULL_USER
                : ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE);
    }

    private String screenBadge() {
        DisplayMetrics dm = getResources().getDisplayMetrics();
        int sw = getResources().getConfiguration().smallestScreenWidthDp;
        String mode = sw >= 600 ? "Unfolded / Large" : "Cover / Phone";
        return Build.MODEL + " · " + dm.widthPixels + "x" + dm.heightPixels
                + " (" + sw + " dp · " + mode + ")";
    }

    private void show(String note) {
        box = Screen.page(this, game.title, screenBadge());

        message = Screen.text(this, note == null ? "" : note, 14, Screen.RED);
        message.setVisibility(note == null ? View.GONE : View.VISIBLE);
        box.addView(message);

        boolean installed = game.isInstalled(this);
        box.addView(Screen.heading(this, "Game Files"));
        box.addView(Screen.text(this,
                installed
                        ? "Game files are installed in private app storage and ready to launch."
                        : "Would you like to download the rest of the game files? Sign in to Dropbox "
                                + "to download the prepared game files, or pick a local folder.",
                15, Screen.TEXT));

        box.addView(Screen.button(this,
                installed ? "Check / Update Game Files from Dropbox" : "Download the game from Dropbox",
                Screen.GREEN,
                v -> download()));

        progress = new TextView(this);
        progress.setTextSize(14);
        progress.setTextColor(Screen.GOLD);
        progress.setPadding(0, Screen.dp(this, 8), 0, 4);
        progress.setVisibility(View.GONE);
        box.addView(progress);

        bar = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
        bar.setMax(1000);
        bar.setVisibility(View.GONE);
        box.addView(bar, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, Screen.dp(this, 14)));

        box.addView(Screen.button(this,
                installed ? "Play " + game.shortLabel : "Launch Runtime / Diagnostics",
                Screen.GOLD,
                v -> launchRuntime()));

        box.addView(Screen.button(this, "Choose local game folder", Screen.SECONDARY, v -> pickFolder()));

        showSyncSection();

        if (DropboxAuth.signedIn(this)) {
            box.addView(Screen.button(this,
                    "Sign out of Dropbox (ends save sync & download)",
                    Screen.SECONDARY,
                    v -> {
                        if (GameDownload.running) {
                            say("Wait for the download to finish first.", true);
                            return;
                        }
                        boolean done = DropboxAuth.signOutAll(this);
                        show(null);
                        say(done
                                ? "Signed out of Dropbox on this device."
                                : "Could not remove all Dropbox credentials. Try again.", !done);
                    }));
        }

        String ver = buildVersion();
        TextView footer = Screen.text(this,
                getPackageName() + (ver.isEmpty() ? "" : " · " + ver),
                12, Screen.MUTED);
        footer.setGravity(Gravity.CENTER_HORIZONTAL);
        footer.setPadding(0, Screen.dp(this, 14), 0, 0);
        box.addView(footer);
    }

    private void showSyncSection() {
        int state = Relay.state(this);
        box.addView(Screen.heading(this, "Save Sync (Dropbox · Save Relay)"));
        String desc;
        if (state == Relay.ON) {
            desc = "On: saves sync with your Dropbox via Save Relay before launch and on exit.";
        } else if (state == Relay.SIGN_IN_AGAIN) {
            desc = "Dropbox needs a fresh sign-in on this device before syncing saves.";
        } else if (state == Relay.BROKEN) {
            desc = "Save sync settings could not be read. Sign in again or turn save sync off.";
        } else {
            desc = "Off. Sign in with Dropbox to sync your Uncharted saves across devices.";
        }
        box.addView(Screen.text(this, desc, 14, Screen.TEXT));

        if (state == Relay.SIGN_IN_AGAIN || state == Relay.BROKEN) {
            box.addView(Screen.button(this, "Sign in to Dropbox again", Screen.BLUE, v -> setUpSync()));
        } else if (state == Relay.OFF) {
            box.addView(Screen.button(this, "Set up save sync (Dropbox)", Screen.BLUE, v -> setUpSync()));
        } else if (state == Relay.ON) {
            box.addView(Screen.button(this, "Sync saves now", Screen.BLUE, v -> syncNow()));
        }

        if (state != Relay.OFF) {
            box.addView(Screen.button(this,
                    "Turn off save sync (keeps download sign-in)",
                    Screen.SECONDARY,
                    v -> {
                        boolean done = Relay.turnOff(this);
                        show(null);
                        say(done ? "Save sync is off." : "Could not remove all save sync settings.", !done);
                    }));
        }
    }

    private void say(String text, boolean error) {
        if (message == null) return;
        message.setTextColor(error ? Screen.RED : Screen.GREEN_TEXT);
        message.setText(text);
        message.setVisibility(View.VISIBLE);
    }

    private void download() {
        if (GameDownload.running) {
            if (DropboxAuth.signedIn(this)) return;
            try {
                String url = DropboxAuth.start(this);
                startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
            } catch (Exception e) {
                say("Dropbox sign-in could not start: " + e.getMessage(), true);
            }
            return;
        }
        if (Build.VERSION.SDK_INT >= 33 && !askedNotifications
                && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                        != PackageManager.PERMISSION_GRANTED) {
            askedNotifications = true;
            requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS}, REQ_NOTIFICATIONS);
            return;
        }
        String url = null;
        if (!DropboxAuth.signedIn(this)) {
            try {
                url = DropboxAuth.start(this);
            } catch (Exception e) {
                say("Dropbox sign-in could not start: " + e.getMessage(), true);
                return;
            }
        }
        GameDownload.error = null;
        GameDownload.status = "Starting download...";
        GameDownload.running = true;
        try {
            GameDownload.start(this);
        } catch (Exception e) {
            GameDownload.running = false;
            say("Download could not start: " + e.getMessage(), true);
            return;
        }
        if (url != null) {
            say("Sign in to Dropbox in your browser, allow access, then return to " + game.shortLabel + ".", false);
            startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
        }
        tick();
    }

    @Override
    public void onRequestPermissionsResult(int request, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(request, permissions, results);
        if (request == REQ_NOTIFICATIONS) {
            download();
        }
    }

    private void tick() {
        if (progress != null) {
            String err = GameDownload.error;
            if (GameDownload.running) {
                long all = GameDownload.total;
                long have = GameDownload.done;
                progress.setText((all > 0 ? (have * 100 / all) + "% · " : "") + GameDownload.status);
                progress.setVisibility(View.VISIBLE);
                bar.setIndeterminate(all <= 0);
                if (all > 0) bar.setProgress((int) (have * 1000 / all));
                bar.setVisibility(View.VISIBLE);
            } else if (err != null) {
                progress.setText(err);
                progress.setVisibility(View.VISIBLE);
                bar.setVisibility(View.GONE);
            } else if (GameDownload.downloaded(this) && GameDownload.finished) {
                GameDownload.finished = false;
                show(null);
                say("Game files downloaded and verified!", false);
                return;
            }
        }
        ticks.postDelayed(this::tick, 500);
    }

    private void setUpSync() {
        if (busy || GameDownload.running) return;
        busy = true;
        try {
            String url = DropboxAuth.start(this);
            say("Sign in to Dropbox in your browser, then return here to enable save sync.", false);
            startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
            new Thread(() -> {
                String err = DropboxAuth.await(DropboxAuth.current());
                runOnUiThread(() -> {
                    busy = false;
                    if (isFinishing()) return;
                    show(null);
                    if (err == null) {
                        say("Save sync is on!", false);
                    } else {
                        say("Dropbox sign-in: " + err, true);
                    }
                });
            }, "save-sync-setup").start();
        } catch (Exception e) {
            busy = false;
            say("Dropbox sign-in could not start: " + e.getMessage(), true);
        }
    }

    private void syncNow() {
        if (busy) return;
        busy = true;
        say("Syncing saves with Dropbox...", false);
        new Thread(() -> {
            JSONObject res = Relay.pullSaves(this, game, null);
            runOnUiThread(() -> handleSyncResult(res, false));
        }, "save-sync-pull").start();
    }

    private void launchRuntime() {
        if (busy) return;
        if (Relay.state(this) != Relay.ON) {
            startActivity(new Intent(this, NativeActivity.class));
            return;
        }
        busy = true;
        say("Pulling cloud saves before launch...", false);
        new Thread(() -> {
            JSONObject res = Relay.pullSaves(this, game, null);
            runOnUiThread(() -> handleSyncResult(res, true));
        }, "save-pull-launch").start();
    }

    private void handleSyncResult(JSONObject res, boolean launchAfter) {
        busy = false;
        if (isFinishing()) return;
        if (res.optBoolean("ok")) {
            JSONObject data = res.optJSONObject("data");
            String outcome = data != null ? data.optString("outcome", "done") : "done";
            if ("conflict".equals(outcome)) {
                showConflictDialog(launchAfter);
                return;
            }
            say("Save sync: " + outcome, false);
            if (launchAfter) {
                startActivity(new Intent(this, NativeActivity.class));
            }
            return;
        }
        JSONObject err = res.optJSONObject("error");
        String msg = err != null ? err.optString("message", "Save sync failed") : "Save sync failed";
        if (launchAfter) {
            new AlertDialog.Builder(this)
                    .setTitle("Save Sync Notice")
                    .setMessage(msg + "\n\nLaunch anyway using local saves?")
                    .setPositiveButton("Launch Anyway", (d, w) ->
                            startActivity(new Intent(this, NativeActivity.class)))
                    .setNegativeButton("Cancel", null)
                    .show();
        } else {
            say("Save sync: " + msg, true);
        }
    }

    private void showConflictDialog(boolean launchAfter) {
        new AlertDialog.Builder(this)
                .setTitle("Save Conflict")
                .setMessage("Cloud saves and local saves differ. Which copy should be kept?")
                .setPositiveButton("Keep Cloud", (d, w) -> resolveConflict("keep-server", launchAfter))
                .setNeutralButton("Keep Local", (d, w) -> resolveConflict("keep-local", launchAfter))
                .setNegativeButton("Cancel", null)
                .show();
    }

    private void resolveConflict(String resolve, boolean launchAfter) {
        busy = true;
        new Thread(() -> {
            JSONObject res = Relay.pullSaves(this, game, resolve);
            runOnUiThread(() -> handleSyncResult(res, launchAfter));
        }, "save-sync-resolve").start();
    }

    private void pickFolder() {
        Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        startActivityForResult(i, PICK_FOLDER);
    }

    @Override
    protected void onActivityResult(int req, int res, Intent data) {
        super.onActivityResult(req, res, data);
        if (req == PICK_FOLDER && res == RESULT_OK && data != null && data.getData() != null) {
            Uri uri = data.getData();
            try {
                getContentResolver().takePersistableUriPermission(
                        uri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
            } catch (Exception ignored) {
            }
            try {
                File dir = game.gameDir(this);
                dir.mkdirs();
                try (FileOutputStream out = new FileOutputStream(new File(dir, ".tree_uri"))) {
                    out.write(uri.toString().getBytes(StandardCharsets.UTF_8));
                }
                say("Selected local folder: " + uri, false);
            } catch (Exception e) {
                say("Could not save selected folder: " + e.getMessage(), true);
            }
        }
    }

    private String buildVersion() {
        try {
            PackageInfo info = getPackageManager().getPackageInfo(getPackageName(), 0);
            String name = info.versionName != null ? info.versionName : "";
            long code = info.getLongVersionCode();
            return name.isEmpty() ? ("#" + code) : (name + " (" + code + ")");
        } catch (Exception e) {
            return "";
        }
    }
}
