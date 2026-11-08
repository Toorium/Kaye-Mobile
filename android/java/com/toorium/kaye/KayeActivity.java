package com.toorium.kaye;

import android.app.AlertDialog;
import android.app.NativeActivity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.text.InputType;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.view.inputmethod.EditorInfo;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.Toast;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.HashMap;

/**
 * Kaye's window on Android. Kaye itself is Rust, in libkaye_mobile.so, and this only does
 * what has to be done from Java: Android's file picker, a name box with the keyboard, and
 * copying files to and from the places the person picks. See src/android.rs.
 */
public class KayeActivity extends NativeActivity {
    static {
        System.loadLibrary("c++_shared");
        System.loadLibrary("kaye_mobile");
    }

    /** Hands an answer back to the Rust thread waiting for it. Null means cancelled. */
    private static native void answer(long id, String[] answer);

    private static final int PICK = 1;
    private static final int SAVE = 2;

    /** What each open picker is for, by its request code. */
    private final HashMap<Integer, long[]> pickIds = new HashMap<>();
    private final HashMap<Integer, String> pickFolders = new HashMap<>();
    private final HashMap<Integer, String> saveSources = new HashMap<>();
    private int nextCode = 100;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        hideSystemBars();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            hideSystemBars();
        }
    }

    /** The whole screen is Kaye's. A swipe from the edge brings the bars back for a moment. */
    private void hideSystemBars() {
        View decor = getWindow().getDecorView();
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            WindowInsetsController controller = decor.getWindowInsetsController();
            if (controller != null) {
                controller.hide(WindowInsets.Type.systemBars());
                controller.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
            }
        } else {
            decor.setSystemUiVisibility(View.SYSTEM_UI_FLAG_FULLSCREEN | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN);
        }
    }

    /** Back leaves Kaye running, the way Home does, so unsaved work isn't closed by accident. */
    @Override
    public void onBackPressed() {
        moveTaskToBack(true);
    }

    // Called from Rust.

    public void pickFiles(final long id, final boolean multiple, final String folder) {
        runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("*/*");
            intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, multiple);
            int code = nextCode++;
            pickIds.put(code, new long[] {id});
            pickFolders.put(code, folder);
            try {
                startActivityForResult(intent, code * 4 + PICK);
            } catch (ActivityNotFoundException e) {
                pickIds.remove(code);
                pickFolders.remove(code);
                answer(id, null);
            }
        });
    }

    public void saveCopy(final String source, final String name) {
        runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("application/octet-stream");
            intent.putExtra(Intent.EXTRA_TITLE, name);
            int code = nextCode++;
            saveSources.put(code, source);
            try {
                startActivityForResult(intent, code * 4 + SAVE);
            } catch (ActivityNotFoundException e) {
                saveSources.remove(code);
                toast("This phone has no app for saving files.");
            }
        });
    }

    public void setClipboard(final String text) {
        runOnUiThread(() -> {
            ClipboardManager clipboard = (ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
            if (clipboard != null) {
                clipboard.setPrimaryClip(ClipData.newPlainText("Kaye", text));
            }
        });
    }

    public void askText(final long id, final boolean numeric, final String title, final String initial, final String ok) {
        runOnUiThread(() -> {
            final EditText field = new EditText(this);
            field.setInputType(numeric
                    ? InputType.TYPE_CLASS_NUMBER | InputType.TYPE_NUMBER_FLAG_DECIMAL | InputType.TYPE_NUMBER_FLAG_SIGNED
                    : InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
            // In landscape the keyboard takes most of the screen, so it shows the text in its
            // own full-width editor above the keys, which is how phones type sideways.
            field.setImeOptions(EditorInfo.IME_ACTION_DONE);
            field.setSingleLine(true);
            field.setHint(title);
            field.setMinHeight((int) (48 * getResources().getDisplayMetrics().density));
            field.setText(initial);
            field.selectAll();
            FrameLayout box = new FrameLayout(this);
            int margin = (int) (20 * getResources().getDisplayMetrics().density);
            FrameLayout.LayoutParams params = new FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.WRAP_CONTENT);
            params.leftMargin = margin;
            params.rightMargin = margin;
            box.addView(field, params);
            final boolean[] answered = {false};
            // No title, which would squeeze the field out when the keyboard is up.
            final AlertDialog dialog = new AlertDialog.Builder(this)
                    .setView(box)
                    .setPositiveButton(ok, (d, which) -> {
                        answered[0] = true;
                        answer(id, new String[] {field.getText().toString()});
                    })
                    .setNegativeButton("Cancel", null)
                    .create();
            // The keyboard's Done key answers the same as the button.
            field.setOnEditorActionListener((view, action, event) -> {
                if (!answered[0]) {
                    answered[0] = true;
                    answer(id, new String[] {field.getText().toString()});
                    dialog.dismiss();
                }
                return true;
            });
            dialog.setOnDismissListener(d -> {
                if (!answered[0]) {
                    answered[0] = true;
                    answer(id, null);
                }
            });
            dialog.getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_STATE_VISIBLE | WindowManager.LayoutParams.SOFT_INPUT_ADJUST_PAN);
            dialog.show();
            field.requestFocus();
        });
    }

    public void openUrl(final String url) {
        runOnUiThread(() -> {
            try {
                startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
            } catch (ActivityNotFoundException e) {
                toast("This phone has no browser to open " + url);
            }
        });
    }

    public void toast(final String text) {
        runOnUiThread(() -> Toast.makeText(this, text, Toast.LENGTH_LONG).show());
    }

    // Answers from the pickers.

    @Override
    protected void onActivityResult(int requestCode, int resultCode, final Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        final int code = requestCode / 4;
        final boolean ok = resultCode == RESULT_OK && data != null;
        if (requestCode % 4 == PICK && pickIds.containsKey(code)) {
            final long id = pickIds.remove(code)[0];
            final String folder = pickFolders.remove(code);
            if (!ok) {
                answer(id, null);
                return;
            }
            final ArrayList<Uri> uris = new ArrayList<>();
            if (data.getClipData() != null) {
                for (int i = 0; i < data.getClipData().getItemCount(); i++) {
                    uris.add(data.getClipData().getItemAt(i).getUri());
                }
            } else if (data.getData() != null) {
                uris.add(data.getData());
            }
            // Copying a video can take a while, so it's off the window's thread.
            new Thread(() -> {
                ArrayList<String> paths = new ArrayList<>();
                for (Uri uri : uris) {
                    String path = copyIn(uri, folder);
                    if (path != null) {
                        paths.add(path);
                    }
                }
                answer(id, paths.isEmpty() ? null : paths.toArray(new String[0]));
            }).start();
        } else if (requestCode % 4 == SAVE && saveSources.containsKey(code)) {
            final String source = saveSources.remove(code);
            if (!ok || data.getData() == null) {
                return;
            }
            final Uri uri = data.getData();
            new Thread(() -> {
                try (InputStream in = new FileInputStream(source); OutputStream out = getContentResolver().openOutputStream(uri, "wt")) {
                    copy(in, out);
                    toast("Saved " + new File(source).getName());
                } catch (Exception e) {
                    toast("That file couldn't be saved: " + e.getMessage());
                }
            }).start();
        }
    }

    /** Copies a picked file into Kaye's folder, keeping its name, and gives back the copy's path. */
    private String copyIn(Uri uri, String folder) {
        String name = "file";
        try (Cursor cursor = getContentResolver().query(uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst() && !cursor.isNull(0)) {
                name = cursor.getString(0);
            }
        } catch (Exception e) {
            // The name is only nice to have.
        }
        name = name.replaceAll("[\\\\/:*?\"<>|]", "_");
        File dir = new File(folder);
        dir.mkdirs();
        // A project of the same name may be saved work, so it's never written over.
        File target = new File(dir, name);
        int dot = name.lastIndexOf('.');
        String stem = dot > 0 ? name.substring(0, dot) : name;
        String extension = dot > 0 ? name.substring(dot) : "";
        for (int n = 2; target.exists(); n++) {
            target = new File(dir, stem + " (" + n + ")" + extension);
        }
        try (InputStream in = getContentResolver().openInputStream(uri); OutputStream out = new FileOutputStream(target)) {
            if (in == null) {
                return null;
            }
            copy(in, out);
            return target.getAbsolutePath();
        } catch (Exception e) {
            toast(name + " couldn't be opened: " + e.getMessage());
            return null;
        }
    }

    private static void copy(InputStream in, OutputStream out) throws java.io.IOException {
        byte[] buffer = new byte[1 << 16];
        int read;
        while ((read = in.read(buffer)) > 0) {
            out.write(buffer, 0, read);
        }
    }
}
