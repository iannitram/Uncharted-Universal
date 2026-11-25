package org.uncharted.universal;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Context;
import android.content.pm.PackageManager;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.Binder;
import android.os.Process;

/**
 * Signature-protected ContentProvider exposing this app's Dropbox Save Relay
 * refresh token to sibling apps (Uncharted 1/2/3 and compatible apps like ACNH)
 * signed with the same signing key, so the player only logs in once per device.
 */
public final class SharedTokenProvider extends ContentProvider {
    static final String[] COLUMNS = {"user", "refresh_token"};

    @Override
    public boolean onCreate() {
        return true;
    }

    @Override
    public Cursor query(
            Uri uri,
            String[] projection,
            String selection,
            String[] selectionArgs,
            String sortOrder) {
        Context ctx = getContext();
        MatrixCursor cursor = new MatrixCursor(COLUMNS);
        if (ctx == null || uri == null || !"/dropbox".equals(uri.getPath())) {
            return cursor;
        }
        int callingUid = Binder.getCallingUid();
        if (callingUid != Process.myUid()) {
            int sig = ctx.getPackageManager().checkSignatures(Process.myUid(), callingUid);
            if (sig != PackageManager.SIGNATURE_MATCH) {
                return cursor;
            }
        }
        String user = Relay.user(ctx);
        String refresh = Relay.dropboxRefresh(ctx);
        if (refresh != null && !refresh.isEmpty()) {
            cursor.addRow(new Object[]{user == null ? "" : user, refresh});
        }
        return cursor;
    }

    @Override
    public String getType(Uri uri) {
        return "vnd.android.cursor.item/vnd.saverelay.dropbox.token";
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        return null;
    }

    @Override
    public int delete(Uri uri, String selection, String[] selectionArgs) {
        return 0;
    }

    @Override
    public int update(Uri uri, ContentValues values, String selection, String[] selectionArgs) {
        return 0;
    }
}
