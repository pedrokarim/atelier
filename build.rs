//! Ressources Windows du binaire : icône de fichier, manifeste et métadonnées.
//!
//! Sans icône de ressource, l'exécutable porte l'icône générique de Windows ;
//! l'icône embarquée par `include_bytes!` ne sert qu'à la zone de notification.

fn main() {
    if !std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        return;
    }

    println!("cargo:rerun-if-changed=assets/atelier.ico");
    println!("cargo:rerun-if-changed=assets/atelier.manifest");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/atelier.ico");
    resource.set_manifest_file("assets/atelier.manifest");
    resource.set("FileDescription", "Atelier");
    resource.set("ProductName", "Atelier");
    resource.set("CompanyName", "Ascencia");
    resource.set("LegalCopyright", "© 2026 Ascencia");
    resource.set("OriginalFilename", "atelier.exe");

    // Un échec ici ne doit pas empêcher de compiler : le binaire reste
    // fonctionnel, seulement sans icône ni styles visuels.
    if let Err(error) = resource.compile() {
        println!("cargo:warning=ressources Windows non embarquées : {error}");
    }
}
