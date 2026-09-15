As of right now, COSMIC Files does not natively support third-party context menu extensions, plugins, or user scripts. System76 is building the [COSMIC Files](https://github.com/pop-os/cosmic-files) manager entirely from scratch in Rust, and a public API or a "scripts directory" to inject custom items into the right-click menu is still an open feature request. [1, 2, 3] 
However, because you are building an app, you can achieve the exact user workflow you want using standard Linux desktop standards.
------------------------------
## The Native Workaround: "Open With..." Integration
Until COSMIC Files opens up an explicit extension API, the standard way for an app to handle files from the OS file manager is to register as a handler for those files. When a user right-clicks a file, your app will appear under the "Open With..." or "Open with other application" sub-menu. [4] 
## Step 1: Update your .desktop configuration
To make your app visible to COSMIC Files when a user right-clicks a file, you need to configure your app's com.yourname.yourapp.desktop file to accept file targets.
Add the %F or %u variable to your Exec line, and define the MimeTypes your app supports:

[Desktop Entry]
Type=Application
Name=Your Cosmic Share App
Comment=Share files instantly
# %F passes a list of local file paths to your app when clicked
Exec=your-share-app-binary %F
Icon=com.yourname.yourapp
Terminal=false
# Tell COSMIC Files to show your app for ALL file types, or specific ones (e.g., image/png)
MimeType=all/allfiles;
Categories=Utility;

## Step 2: Read the file paths in your Rust / COSMIC Code
When the user right-clicks a file in COSMIC Files and selects Open With > Your Cosmic Share App, COSMIC launches your binary and passes the absolute file path as a CLI argument.
In your app's initialization logic (usually inside your main.rs before starting your iced / cosmic window loop), read the system arguments:

```Rust
use std::env;use std::path::PathBuf;
fn main() {
    // Collect paths passed by COSMIC Files
    let args: Vec<String> = env::args().collect();
    
    // The first arg is the binary path; subsequent args are the target files
    let files_to_share: Vec<PathBuf> = args.iter()
        .skip(1)
        .map(PathBuf::from)
        .collect();

    if !files_to_share.is_empty() {
        // Launch your COSMIC App window pre-loaded with these files in the "Share" state
        println!("Files to share: {:?}", files_to_share);
        
        // Pass 'files_to_share' into your App's flags/initial state
        // run_cosmic_app(files_to_share); 
    } else {
        // Launch app normally if opened without right-clicking a file
        // run_cosmic_app(vec![]);
    }
}
```

## The Long-Term Solution (Tracking Upstream)
If you specifically want a dedicated, standalone "Share" button rather than an "Open With" target, keep an eye on the official upstream project. You can track [COSMIC Files Issue #1445 on GitHub](https://github.com/pop-os/cosmic-files/issues/1445) and [Issue #1368](https://github.com/pop-os/cosmic-files/issues/1368), which are tracking the implementation of user scripts and custom context menu configurations. Once that lands, you will be able to ship a small script wrapper with your application that injects a direct "Share with..." button.

## Notably

We could benefit from having aan android/quickshare/localsend type of integration, where we can select a node on the tailnet, select share, then drag and drop files in an area. In that area, of course there would also be a `select files` button, which opens a file dialog for the user to select one or more files. This may be the cleanest way for now.

[1] [https://github.com](https://github.com/pop-os/cosmic-files/issues/1445)
[2] [https://github.com](https://github.com/pop-os/cosmic-files/issues/1445)
[3] [https://github.com](https://github.com/pop-os/cosmic-files/issues/1368)
[4] [https://github.com](https://github.com/pop-os/cosmic-files/issues/322)
