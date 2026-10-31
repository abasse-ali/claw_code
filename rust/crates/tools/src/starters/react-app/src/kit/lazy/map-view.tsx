// La carte elle-même (Leaflet et les fonds de carte d'OpenStreetMap). Chargée seulement quand
// une MapView s'affiche : une appli sans carte ne la télécharge pas. Passez par MapView.

import L from "leaflet";
import "./map-view.css";
import { useEffect, useRef } from "react";
import type { MapViewProps } from "../map";

export default function MapViewImpl({ center, zoom = 13, markers = [], route, onPick, onMarker, fit = true, height = 280, label }: MapViewProps) {
  const box = useRef<HTMLDivElement>(null);
  const map = useRef<L.Map | null>(null);
  const layer = useRef<L.LayerGroup | null>(null);
  const handlers = useRef({ onPick, onMarker });
  handlers.current = { onPick, onMarker };

  useEffect(() => {
    if (!box.current) return;
    const created = L.map(box.current, { zoomControl: true, attributionControl: true }).setView([center.lat, center.lng], zoom);
    L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
      maxZoom: 19,
      attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>',
    }).addTo(created);
    created.on("click", (event: L.LeafletMouseEvent) => handlers.current.onPick?.({ lat: event.latlng.lat, lng: event.latlng.lng }));
    layer.current = L.layerGroup().addTo(created);
    map.current = created;
    // Une carte dans un écran qui vient d'apparaître : sa taille se mesure une fois posée.
    const resize = new ResizeObserver(() => created.invalidateSize());
    resize.observe(box.current);
    return () => { resize.disconnect(); created.remove(); map.current = null; };
    // La carte se crée une fois ; le centre et le zoom suivent ensuite (effet suivant).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (map.current && !(fit && markers.length > 1)) map.current.setView([center.lat, center.lng], zoom);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [center.lat, center.lng, zoom]);

  // Les épingles et le trajet ne sont redessinés (et recadrés) que s'ils changent vraiment :
  // un nouveau tableau aux mêmes lieux ne défait pas le zoom de l'utilisateur.
  const drawn = JSON.stringify([markers, route ?? null, fit]);
  useEffect(() => {
    const group = layer.current;
    if (!group || !map.current) return;
    group.clearLayers();
    for (const marker of markers) {
      const pin = L.marker([marker.lat, marker.lng], {
        title: marker.label,
        alt: marker.label,
        keyboard: true,
        icon: L.divIcon({ className: `map-pin map-pin--${marker.tone ?? "accent"}`, html: "<i></i>", iconSize: [28, 28], iconAnchor: [14, 28], popupAnchor: [0, -26] }),
      });
      if (marker.label) pin.bindPopup(marker.note ? `<b>${escape(marker.label)}</b><br>${escape(marker.note)}` : `<b>${escape(marker.label)}</b>`);
      pin.on("click", () => handlers.current.onMarker?.(marker.id));
      group.addLayer(pin);
    }
    if (route && route.length > 1) group.addLayer(L.polyline(route.map((point) => [point.lat, point.lng] as [number, number]), { weight: 5, opacity: 0.85, className: "map-route" }));
    const points = [...markers, ...(route ?? [])];
    if (fit && points.length > 1) map.current.fitBounds(L.latLngBounds(points.map((point) => [point.lat, point.lng] as [number, number])), { padding: [32, 32], maxZoom: 16 });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drawn]);

  return <div ref={box} className="map-view" style={{ height }} role="region" aria-label={label} />;
}

const escape = (text: string) => text.replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);
