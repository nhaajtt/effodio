// Who is in the photos: name a face once and its name finds every photo of that person; or give a sample photo
// and get the photos with a face like the one in it.

import * as api from "./api";
import { Hit } from "./api";
import { t } from "./i18n";
import { report } from "./results";
import { h, icon, toast } from "./ui";

interface Face {
  face: number;
  name: string | null;
  crop: string | null;
}

interface Status {
  models: boolean;
  faces: number;
  people: number;
  scanned: number;
  photos: number;
}

interface People {
  named: { person: number; name: string; photos: number; faces: Face[] }[];
  unnamed: { photos: number; group: number[]; faces: Face[] }[];
}

const portrait = (face: Face, size = 56) =>
  face.crop ? h("img.face", { alt: "", src: api.fileUrl(face.crop), style: { width: `${size}px`, height: `${size}px` } }) : h("span.face", { style: { width: `${size}px`, height: `${size}px` } });

/** A box to type a name in; `save` gets the name when Enter or the button is pressed. */
function nameBox(save: (name: string) => void, value = ""): HTMLElement {
  const input = h("input", { type: "text", placeholder: t("people.name"), value, spellcheck: false });
  const go = () => {
    const name = input.value.trim();
    if (name) save(name);
  };
  input.addEventListener("keydown", (event) => {
    event.stopPropagation();
    if (event.key === "Enter") go();
  });
  return h("div.name-box", input, h("button.button.primary", { onclick: go }, t("people.save")));
}

export async function mountPeople(container: HTMLElement, actions: { search(query: string): void; byFace(path: string): void }): Promise<void> {
  const body = h("div.groups", h("div.empty", h("p", "…")));
  container.replaceChildren(h("div.view", h("div.heading", h("h2", t("people.title")), h("p", t("people.body"))), body));

  const draw = async () => {
    const settings = await api.getConfig();
    const status = await api.ask<Status>({ op: "faces_status" });
    if (!settings.faces || !status.models) {
      const turnOn = h("button.button.primary", icon("people", 16), t("people.on"));
      turnOn.addEventListener("click", async () => {
        turnOn.disabled = true;
        turnOn.textContent = t("people.working");
        try {
          await api.ask({ op: "faces_enable" });
          await api.setConfig({ ...(await api.getConfig()), faces: true });
          await api.indexNow();
          await draw();
        } catch (error) {
          report(error);
          turnOn.disabled = false;
          turnOn.textContent = t("people.on");
        }
      });
      body.replaceChildren(h("div.empty", h("div.orb.glass", icon("people", 30)), h("p", t("people.off")), h("p.small", t("people.offBody")), turnOn));
      return;
    }
    const people = await api.ask<People>({ op: "people" });
    const sample = h("button.button", icon("image", 16), t("people.sample"));
    sample.addEventListener("click", async () => {
      const path = await api.pickExample();
      if (path) actions.byFace(path);
    });
    const wipe = h("button.button.quiet", icon("trash", 15), t("people.wipe"));
    wipe.addEventListener("click", async () => {
      // A second press within a few seconds does it: this can't be undone.
      if (wipe.dataset.armed !== "1") {
        wipe.dataset.armed = "1";
        wipe.lastChild!.textContent = t("people.wipeSure");
        setTimeout(() => {
          wipe.dataset.armed = "";
          wipe.lastChild!.textContent = t("people.wipe");
        }, 4000);
        return;
      }
      await api.ask({ op: "faces_wipe" });
      await api.setConfig({ ...(await api.getConfig()), faces: false });
      await draw();
    });
    const bar = h("div.people-bar", h("span.pill", t("people.progress", { done: status.scanned, total: status.photos })), h("span.grow"), sample, wipe);
    const parts: HTMLElement[] = [bar];
    if (people.named.length) {
      parts.push(
        h(
          "div.group-card.glass",
          h("header", t("people.named")),
          h(
            "div.people",
            ...people.named.map((person) => {
              const forget = h("span.remove.icon-button", { title: t("people.forget") }, icon("close", 12));
              forget.addEventListener("click", (event) => {
                event.stopPropagation();
                api.ask({ op: "person_forget", person: person.person }).then(draw, report);
              });
              return h(
                "button.person",
                { onclick: () => actions.search(`person:"${person.name}"`) },
                h("div.faces", ...person.faces.slice(0, 3).map((face) => portrait(face))),
                h("b", person.name),
                h("span", t("people.photos", { n: person.photos })),
                forget,
              );
            }),
          ),
        ),
      );
    }
    if (people.unnamed.length) {
      parts.push(
        h(
          "div.group-card.glass",
          h("header", t("people.unnamed")),
          h(
            "div.people",
            ...people.unnamed.map((group) =>
              h(
                "div.person",
                h("div.faces", ...group.faces.slice(0, 3).map((face) => portrait(face))),
                h("span", t("people.photos", { n: group.photos })),
                nameBox((name) => {
                  api.ask<{ spread: number }>({ op: "face_name", face: group.group[0], name, group: group.group }).then((result) => {
                    toast(t("people.spread", { n: result.spread }));
                    void draw();
                  }, report);
                }),
              ),
            ),
          ),
        ),
      );
    }
    if (!people.named.length && !people.unnamed.length) parts.push(h("div.empty", h("div.orb.glass", icon("people", 30)), h("p", t("people.none"))));
    body.replaceChildren(...parts);
  };

  try {
    await draw();
  } catch (error) {
    report(error);
  }
}

/** Under a photo's preview: the faces in it. A face without a name can be given one here. */
export function facesOf(hit: Hit): HTMLElement | null {
  if (hit.kind !== "image") return null;
  const row = h("div.faces-row");
  const draw = async () => {
    const settings = await api.getConfig();
    if (!settings.faces) return;
    const { faces } = await api.ask<{ faces: Face[] }>({ op: "faces_of", file: hit.file });
    row.replaceChildren(
      ...faces.map((face) => {
        const label = h("span", face.name ?? t("face.who"));
        const chip = h("button.face-chip.glass", { title: face.name ?? t("face.who") }, portrait(face, 30), label);
        chip.addEventListener("click", () => {
          const box = nameBox((name) => {
            api.ask<{ spread: number }>({ op: "face_name", face: face.face, name }).then((result) => {
              toast(t("people.spread", { n: result.spread }));
              void draw();
            }, report);
          }, face.name ?? "");
          chip.replaceWith(box);
          box.querySelector("input")?.focus();
        });
        return chip;
      }),
    );
  };
  draw().catch(() => {});
  return row;
}
