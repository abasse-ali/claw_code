// Liste des recettes ; un clic montre ses ingrédients.
"use strict";

const liste = document.getElementById("liste");
const detail = document.getElementById("detail");

async function charger() {
  const reponse = await fetch("/api/recettes");
  afficher(await reponse.json());
}

function afficher(recettes) {
  liste.replaceChildren();
  for (const recette of recettes) {
    const element = document.createElement("li");
    element.textContent = recette.nom;
    element.addEventListener("click", () => montrer(recette));
    liste.append(element);
  }
}

function montrer(recette) {
  detail.hidden = false;
  detail.replaceChildren();
  const titre = document.createElement("h2");
  titre.textContent = recette.nom;
  const ingredients = document.createElement("ul");
  for (const ingredient of recette.ingredients) {
    const element = document.createElement("li");
    element.textContent = ingredient;
    ingredients.append(element);
  }
  detail.append(titre, ingredients);
}

charger();
