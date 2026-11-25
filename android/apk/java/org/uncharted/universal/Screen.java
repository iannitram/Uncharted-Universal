package org.uncharted.universal;

import android.app.Activity;
import android.content.Context;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.view.Gravity;
import android.view.View;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

final class Screen {
    static final int BG_TOP = Color.rgb(18, 24, 34);
    static final int BG_BOTTOM = Color.rgb(28, 36, 48);
    static final int CARD = Color.rgb(34, 42, 56);
    static final int CARD_BORDER = Color.rgb(66, 82, 108);
    static final int TITLE = Color.rgb(245, 240, 224);
    static final int TEXT = Color.rgb(206, 214, 226);
    static final int MUTED = Color.rgb(148, 162, 184);
    static final int GOLD = Color.rgb(212, 160, 62);
    static final int GREEN = Color.rgb(54, 152, 92);
    static final int BLUE = Color.rgb(52, 118, 198);
    static final int SECONDARY = Color.rgb(54, 66, 86);
    static final int RED = Color.rgb(235, 110, 110);
    static final int GREEN_TEXT = Color.rgb(112, 224, 152);

    private Screen() {}

    static int dp(Context c, float v) {
        return Math.round(v * c.getResources().getDisplayMetrics().density);
    }

    static Button button(Context c, String text, int color, View.OnClickListener click) {
        Button b = new Button(c);
        b.setText(text);
        b.setAllCaps(false);
        b.setTextSize(16);
        b.setTypeface(Typeface.DEFAULT_BOLD);
        b.setTextColor(color == GOLD ? Color.rgb(24, 20, 12) : Color.WHITE);
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(color);
        bg.setCornerRadius(dp(c, 26));
        b.setBackground(bg);
        b.setPadding(dp(c, 20), dp(c, 10), dp(c, 20), dp(c, 10));
        b.setOnClickListener(click);
        b.setMinHeight(dp(c, 52));
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        lp.topMargin = dp(c, 10);
        b.setLayoutParams(lp);
        return b;
    }

    static TextView text(Context c, String s, float size, int color) {
        TextView v = new TextView(c);
        v.setText(s);
        v.setTextSize(size);
        v.setTextColor(color);
        v.setPadding(0, dp(c, 5), 0, dp(c, 5));
        return v;
    }

    static TextView heading(Context c, String s) {
        TextView v = text(c, s, 17, GOLD);
        v.setTypeface(Typeface.DEFAULT_BOLD);
        v.setPadding(0, dp(c, 16), 0, dp(c, 2));
        return v;
    }

    static TextView badge(Context c, String s) {
        TextView v = new TextView(c);
        v.setText(s);
        v.setTextSize(12);
        v.setTextColor(TEXT);
        v.setPadding(dp(c, 10), dp(c, 5), dp(c, 10), dp(c, 5));
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(Color.rgb(44, 56, 74));
        bg.setStroke(dp(c, 1), CARD_BORDER);
        bg.setCornerRadius(dp(c, 10));
        v.setBackground(bg);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        lp.topMargin = dp(c, 6);
        lp.bottomMargin = dp(c, 6);
        v.setLayoutParams(lp);
        return v;
    }

    static LinearLayout page(Activity a, String title, String subtitle) {
        ScrollView scroll = new ScrollView(a);
        scroll.setFillViewport(true);
        GradientDrawable backdrop = new GradientDrawable(
                GradientDrawable.Orientation.TL_BR,
                new int[]{BG_TOP, BG_BOTTOM});
        scroll.setBackground(backdrop);

        LinearLayout outer = new LinearLayout(a);
        outer.setGravity(Gravity.CENTER);
        outer.setPadding(dp(a, 20), dp(a, 24), dp(a, 20), dp(a, 24));

        LinearLayout box = new LinearLayout(a);
        box.setOrientation(LinearLayout.VERTICAL);
        box.setPadding(dp(a, 24), dp(a, 22), dp(a, 24), dp(a, 22));
        GradientDrawable card = new GradientDrawable();
        card.setColor(CARD);
        card.setStroke(dp(a, 1), CARD_BORDER);
        card.setCornerRadius(dp(a, 24));
        box.setBackground(card);

        int maxCardPx = dp(a, 580);
        int availPx = Math.max(dp(a, 260), a.getResources().getDisplayMetrics().widthPixels - dp(a, 40));
        outer.addView(box, new LinearLayout.LayoutParams(
                Math.min(maxCardPx, availPx),
                LinearLayout.LayoutParams.WRAP_CONTENT));
        scroll.addView(outer);

        TextView t = new TextView(a);
        t.setText(title);
        t.setTextSize(24);
        t.setTypeface(Typeface.DEFAULT_BOLD);
        t.setTextColor(TITLE);
        box.addView(t);

        if (subtitle != null && !subtitle.isEmpty()) {
            box.addView(badge(a, subtitle));
        }

        a.setContentView(scroll);
        return box;
    }
}
