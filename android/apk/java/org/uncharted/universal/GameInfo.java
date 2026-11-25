package org.uncharted.universal;

import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.os.Bundle;

import java.io.File;

final class GameInfo {
    final String key;          // u1 | u2 | u3
    final String title;        // Uncharted: Drake's Fortune | ...
    final String shortLabel;   // Uncharted 1 | 2 | 3
    final String manifestJson; // Save Relay manifest JSON
    final String remoteDir;    // /uncharted-universal-game/u1
    final String scheme;       // uncharted-u1

    private GameInfo(
            String key,
            String title,
            String shortLabel,
            String manifestJson,
            String remoteDir,
            String scheme) {
        this.key = key;
        this.title = title;
        this.shortLabel = shortLabel;
        this.manifestJson = manifestJson;
        this.remoteDir = remoteDir;
        this.scheme = scheme;
    }

    static GameInfo forContext(Context c) {
        String key = null;
        try {
            ApplicationInfo ai = c.getPackageManager()
                    .getApplicationInfo(c.getPackageName(), PackageManager.GET_META_DATA);
            Bundle meta = ai.metaData;
            if (meta != null) {
                key = meta.getString("org.uncharted.game_id");
            }
        } catch (Exception ignored) {
        }
        if (key == null || key.isEmpty()) {
            String pkg = c.getPackageName();
            if (pkg.endsWith(".u2")) key = "u2";
            else if (pkg.endsWith(".u3")) key = "u3";
            else key = "u1";
        }
        switch (key) {
            case "u2":
                return new GameInfo(
                        "u2",
                        "Uncharted 2: Among Thieves",
                        "Uncharted 2",
                        "{\"format\":1,\"id\":\"uncharted-2\",\"name\":\"Uncharted 2: Among Thieves\","
                                + "\"saves\":{},\"saves_from\":\"launcher\","
                                + "\"include\":[\"SAVEDATA02*/**\"],"
                                + "\"exclude\":[\"**/*.tmp\",\"**/.save-relay-*\"]}",
                        "/uncharted-universal-game/u2",
                        "uncharted-u2");
            case "u3":
                return new GameInfo(
                        "u3",
                        "Uncharted 3: Drake's Deception",
                        "Uncharted 3",
                        "{\"format\":1,\"id\":\"uncharted-3\",\"name\":\"Uncharted 3: Drake's Deception\","
                                + "\"saves\":{},\"saves_from\":\"launcher\","
                                + "\"include\":[\"SAVEDATA03*/**\"],"
                                + "\"exclude\":[\"**/*.tmp\",\"**/.save-relay-*\"]}",
                        "/uncharted-universal-game/u3",
                        "uncharted-u3");
            default:
                return new GameInfo(
                        "u1",
                        "Uncharted: Drake's Fortune",
                        "Uncharted 1",
                        "{\"format\":1,\"id\":\"uncharted-1\",\"name\":\"Uncharted: Drake's Fortune\","
                                + "\"saves\":{},\"saves_from\":\"launcher\","
                                + "\"include\":[\"SAVEDATA01*/**\"],"
                                + "\"exclude\":[\"**/*.tmp\",\"**/.save-relay-*\"]}",
                        "/uncharted-universal-game/u1",
                        "uncharted-u1");
        }
    }

    File gameDir(Context c) {
        return new File(c.getFilesDir(), "games/" + key);
    }

    File partialDir(Context c) {
        return new File(c.getFilesDir(), "games/" + key + ".partial");
    }

    File savesDir(Context c) {
        File dir = new File(c.getFilesDir(), "saves/" + key);
        if (!dir.exists()) {
            //noinspection ResultOfMethodCallIgnored
            dir.mkdirs();
        }
        return dir;
    }

    boolean isInstalled(Context c) {
        return new File(gameDir(c), ".installed").isFile();
    }
}
